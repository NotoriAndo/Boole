#!/usr/bin/env python3
"""Contract tests for the main-only launcher-v2 v5 workflow."""

from __future__ import annotations

import json
import hashlib
import os
import pathlib
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
import unittest

from scripts import native_shadow_boot_ci_payload_acquire_arm64_v1 as ci_payload
from scripts import native_shadow_boot_rustdist_acquire_arm64_v1 as rustdist
from scripts import native_shadow_boot_writer_set_acquire_arm64_v1 as writer_set
from scripts import native_shadow_boot_writer_tree_arm64_v1 as writer_tree


REPO = pathlib.Path(__file__).resolve().parents[1]
WRAPPER = REPO / "scripts/native-shadow-successor-produce-arm64-v5.sh"
WORKFLOW = REPO / ".github/workflows/native-shadow-successor-produce-arm64-v5.yml"

EXACT_EVENT_NAME = "workflow_dispatch"
EXACT_DISPATCH_REF = "refs/heads/main"
EXACT_WORKFLOW_REF = (
    "NotoriAndo/Boole/.github/workflows/"
    "native-shadow-successor-produce-arm64-v5.yml@refs/heads/main"
)


def workflow_job(name: str) -> str:
    source = WORKFLOW.read_text(encoding="utf-8")
    marker = f"  {name}:\n"
    if marker not in source:
        raise AssertionError(f"workflow job is absent: {name}")
    body = source.split(marker, 1)[1]
    following = re.search(r"^  [a-zA-Z0-9_-]+:\n", body, re.MULTILINE)
    return body[: following.start()] if following else body


class WorkflowTopologyTests(unittest.TestCase):
    def test_every_job_requires_exact_event_main_and_workflow_ref_before_checkout(
        self,
    ) -> None:
        expected_context = (
            "github.event_name == 'workflow_dispatch'",
            "github.ref == 'refs/heads/main'",
            "github.workflow_ref == "
            "'NotoriAndo/Boole/.github/workflows/"
            "native-shadow-successor-produce-arm64-v5.yml@refs/heads/main'",
        )
        expected_mode = {
            "production-authority-guard": "inputs.mode == 'production'",
            "free-rehearsal": "inputs.mode == 'rehearsal'",
            "produce": "inputs.mode == 'production'",
            "compare": "inputs.mode == 'production'",
        }

        for name, mode in expected_mode.items():
            with self.subTest(job=name):
                body = workflow_job(name)
                checkout = body.index("uses: actions/checkout@")
                guard = body[:checkout]
                self.assertIn(mode, guard)
                for expression in expected_context:
                    self.assertIn(expression, guard)

    def test_guard_claim_creation_and_publication_recheck_the_full_live_context(
        self,
    ) -> None:
        guard = workflow_job("production-authority-guard")
        required_arguments = (
            '--github-run-id "$GITHUB_RUN_ID_VALUE"',
            '--github-run-attempt "$GITHUB_RUN_ATTEMPT_VALUE"',
            '--event-name "$GITHUB_EVENT_NAME_VALUE"',
            '--dispatch-ref "$GITHUB_REF_VALUE"',
            '--workflow-path "$WORKFLOW_PATH"',
            '--workflow-ref "$GITHUB_WORKFLOW_REF_VALUE"',
            '--head-sha "$HEAD_SHA"',
            '--head-authority-sha256 "$HEAD_AUTHORITY_SHA256"',
        )

        authority_check = guard[
            guard.index("--verify-production-authority-only") :
            guard.index("Build and locally verify")
        ]
        claim_message = guard[
            guard.index("--dispatch-claim-message") :
            guard.index("attempt_id=", guard.index("--dispatch-claim-message"))
        ]
        local_claim_check = guard[
            guard.index("--verify-dispatch-claim") :
            guard.index("printf 'claim_ref=", guard.index("--verify-dispatch-claim"))
        ]
        publication = guard[guard.index("Publish the claim exactly once") :]

        for block_name, block in (
            ("authority", authority_check),
            ("claim-message", claim_message),
            ("local-claim", local_claim_check),
            ("publication", publication),
        ):
            with self.subTest(consumer=block_name):
                for argument in required_arguments:
                    self.assertIn(argument, block)

        publication_recheck = publication.index("--verify-dispatch-claim")
        self.assertLess(publication_recheck, publication.index("GIT_ASKPASS"))
        self.assertLess(publication_recheck, publication.index("git push"))

    def test_replica_root_and_compare_consumers_receive_the_full_live_context(
        self,
    ) -> None:
        produce = workflow_job("produce")
        compare = workflow_job("compare")
        runner_arguments = (
            '--github-run-id "$GITHUB_RUN_ID_VALUE"',
            '--github-run-attempt "$GITHUB_RUN_ATTEMPT_VALUE"',
            '--event-name "$GITHUB_EVENT_NAME_VALUE"',
            '--dispatch-ref "$GITHUB_REF_VALUE"',
            '--workflow-path "$WORKFLOW_PATH"',
            '--workflow-ref "$GITHUB_WORKFLOW_REF_VALUE"',
            '--head-sha "$HEAD_SHA"',
            '--head-authority-sha256 "$HEAD_AUTHORITY_SHA256"',
        )
        root_arguments = (
            '--event-name "$event_name"',
            '--dispatch-ref "$dispatch_ref"',
            '--workflow-path "$workflow_path"',
            '--workflow-ref "$workflow_ref"',
            '--head-sha "$head_sha"',
            '--head-authority-sha256 "$head_authority_sha256"',
        )

        replica_claim = produce[
            produce.index("--verify-dispatch-claim") :
            produce.index("Complete exact HEAD history")
        ]
        replica_authority = produce[
            produce.index("--verify-production-authority-only") :
            produce.index("Install Rust toolchain")
        ]
        root_step = produce[produce.index("Invoke the root-installed production") :]
        production = root_step[
            root_step.index('production_command=(') :
            root_step.index('cleanup_command=(')
        ]
        cleanup = root_step[
            root_step.index('cleanup_command=(') :
            root_step.index("production_status=0")
        ]
        compare_claim = compare[
            compare.index("--verify-dispatch-claim") :
            compare.index("Download replica one")
        ]
        comparison = compare[compare.index("--compare-provenanced-replicas") :]

        for block_name, block in (
            ("replica-claim", replica_claim),
            ("replica-authority", replica_authority),
            ("compare-claim", compare_claim),
            ("comparison", comparison),
        ):
            with self.subTest(consumer=block_name):
                for argument in runner_arguments:
                    self.assertIn(argument, block)
        for block_name, block in (("production", production), ("cleanup", cleanup)):
            with self.subTest(consumer=block_name):
                for argument in root_arguments:
                    self.assertIn(argument, block)

        root_guard = root_step[: root_step.index("/usr/bin/install -d")]
        for check in (
            '[[ $event_name == workflow_dispatch ]]',
            '[[ $dispatch_ref == refs/heads/main ]]',
            '[[ $workflow_path == .github/workflows/native-shadow-successor-produce-arm64-v5.yml ]]',
            '[[ $workflow_ref == NotoriAndo/Boole/.github/workflows/native-shadow-successor-produce-arm64-v5.yml@refs/heads/main ]]',
            '[[ $head_authority_sha256 =~ ^[0-9a-f]{64}$ ]]',
        ):
            self.assertIn(check, root_guard)

    def test_manual_workflow_has_the_four_exact_jobs_and_two_modes(self) -> None:
        source = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("workflow_dispatch:", source)
        self.assertIn("options: [rehearsal, production]", source)
        trigger = source.split("on:\n", 1)[1].split("\npermissions:\n", 1)[0]
        self.assertEqual(
            re.findall(r"^  ([a-zA-Z0-9_-]+):\n", trigger, re.MULTILINE),
            ["workflow_dispatch"],
        )
        jobs = source.split("jobs:\n", 1)[1]
        self.assertEqual(
            set(re.findall(r"^  ([a-zA-Z0-9_-]+):\n", jobs, re.MULTILINE)),
            {"production-authority-guard", "free-rehearsal", "produce", "compare"},
        )
        for automatic in (
            "pull_request:",
            "pull_request_target:",
            "push:",
            "schedule:",
            "workflow_call:",
            "workflow_run:",
        ):
            self.assertNotIn(automatic, source)

    def test_v5_a7_workflow_rejects_every_legacy_generation_namespace(
        self,
    ) -> None:
        source = WORKFLOW.read_text(encoding="utf-8")
        for required in (
            "native-shadow-successor-produce-arm64-v5.sh",
            "native-shadow-mac3-successor-production-authority-arm64-v7.json",
            "refs/tags/boole-native-shadow-mac3-successor-production-a7-",
            "native-shadow-successor-v5-replica-",
            "/var/lib/boole/native-shadow-successor-v5",
            "boole-nsv5-",
        ):
            with self.subTest(required=required):
                self.assertIn(required, source)
        for forbidden in (
            "native-shadow-successor-produce-arm64-v4.sh",
            "native_shadow_successor_produce_phase_arm64_v4.py",
            "native-shadow-successor-v4-replica-",
            "/var/lib/boole/native-shadow-successor-v4",
            "boole-nsv4-",
            "native-shadow-mac3-successor-production-authority-arm64-v6.json",
            "native-shadow-mac3-successor-image-production-result-arm64-v6.json",
            "boole.native-shadow.mac3.successor-production-authority.arm64.v6",
            "refs/tags/boole-native-shadow-mac3-successor-production-a6-",
            "boole.native-shadow.mac3.successor-production-dispatch-claim.arm64.v1",
            "a6Sha256",
            "--head-a6-sha256",
            "A6_PATH",
            "A6_SCHEMA",
            "head_a6_sha256",
        ):
            with self.subTest(forbidden=forbidden):
                self.assertNotIn(forbidden, source)

    def test_v5_keeps_every_historical_v4_test_method(self) -> None:
        old_source = (
            REPO / "scripts/test_native_shadow_successor_produce_workflow_arm64_v4.py"
        ).read_text(encoding="utf-8")
        new_source = pathlib.Path(__file__).read_text(encoding="utf-8")
        pattern = r"^    def (test_[a-zA-Z0-9_]+)\("
        old_methods = set(re.findall(pattern, old_source, re.MULTILINE))
        new_methods = set(re.findall(pattern, new_source, re.MULTILINE))
        self.assertTrue(old_methods)
        self.assertEqual(old_methods - new_methods, set())

    def test_every_checkout_is_detached_at_exact_head_without_trigger_tags(self) -> None:
        source = WORKFLOW.read_text(encoding="utf-8")
        self.assertEqual(source.count("uses: actions/checkout@"), 4)
        self.assertEqual(source.count("ref: ${{ github.sha }}"), 4)
        self.assertEqual(source.count("fetch-tags: false"), 4)

    def test_production_is_guarded_before_dependencies_and_uses_two_replicas(self) -> None:
        guard = workflow_job("production-authority-guard")
        produce = workflow_job("produce")
        self.assertIn("runs-on: ubuntu-24.04-arm", guard)
        self.assertIn("--verify-production-authority-only", guard)
        for forbidden in (
            "rust-toolchain",
            "payload_acquire",
            "writer_set_acquire",
            "RUNNER_TEMP",
            "mkdir",
            "mktemp",
            "upload-artifact@",
            "continue-on-error",
            "|| true",
        ):
            self.assertNotIn(forbidden, guard)

        self.assertIn("needs: [production-authority-guard]", produce)
        self.assertIn("fail-fast: false", produce)
        self.assertIn("replica: [1, 2]", produce)
        self.assertIn("native_shadow_launcher_emit_arm64_v2.py", produce)
        first_guard = produce.index("--verify-production-authority-only")
        for effect in (
            "rust-toolchain@",
            "native_shadow_boot_rustdist_acquire_arm64_v1.py",
            "native_shadow_boot_ci_payload_acquire_arm64_v1.py",
            "native_shadow_boot_writer_set_acquire_arm64_v1.py",
            "native_shadow_launcher_emit_arm64_v2.py",
            "RUNNER_TEMP/acquire",
        ):
                self.assertLess(first_guard, produce.index(effect))

    def test_guard_is_the_only_writer_and_atomically_claims_one_annotated_tag(self) -> None:
        source = WORKFLOW.read_text(encoding="utf-8")
        guard = workflow_job("production-authority-guard")
        self.assertIn("permissions:\n      contents: write", guard)
        for other in ("free-rehearsal", "produce", "compare"):
            self.assertNotIn("contents: write", workflow_job(other))
        self.assertEqual(source.count("contents: write"), 1)
        for context in (
            "${{ github.run_id }}",
            "${{ github.run_attempt }}",
            "${{ github.repository }}",
            "${{ github.workflow_ref }}",
            "${{ github.sha }}",
        ):
            self.assertIn(context, guard)
        self.assertIn("--dispatch-claim-message", guard)
        self.assertIn("git mktag", guard)
        self.assertIn("git push", guard)
        self.assertIn("git ls-remote --refs", guard)
        self.assertIn("--verify-dispatch-claim", guard)
        self.assertIn("GIT_ASKPASS", guard)
        for forbidden in (
            "git tag -f",
            "git push -f",
            "git push --force",
            "update-ref -d",
            "delete_ref",
            "check-ref-format --normalize",
        ):
            self.assertNotIn(forbidden, guard)

    def test_claim_publisher_parses_one_new_tag_status_before_remote_reread(self) -> None:
        guard = workflow_job("production-authority-guard")
        publisher = guard[guard.index("Publish the claim exactly once") :]
        self.assertIn("git push --atomic --porcelain", publisher)
        self.assertIn("mapfile -t push_status_lines", publisher)
        self.assertIn("$1 == \"*\"", publisher)
        self.assertIn("$2 == expected", publisher)
        self.assertIn("$3 == \"[new tag]\"", publisher)
        self.assertIn('[[ ${#push_status_lines[@]} -eq 1 ]]', publisher)
        self.assertNotIn("expected_push=$'To '", publisher)
        self.assertNotIn('[[ $push_output == "$expected_push" ]]', publisher)
        self.assertLess(
            publisher.index('[[ ${#push_status_lines[@]} -eq 1 ]]'),
            publisher.index('remote_after="$(git ls-remote'),
        )

    def test_every_token_step_refuses_askpass_replacement_and_ambient_git_config(
        self,
    ) -> None:
        bodies = (
            workflow_job("production-authority-guard"),
            workflow_job("produce"),
            workflow_job("compare"),
        )
        for body in bodies:
            with self.subTest(job=body.splitlines()[0] if body else "empty"):
                self.assertIn('[[ ! -e $askpass && ! -L $askpass ]]', body)
                self.assertIn("set -o noclobber", body)
                self.assertIn("GIT_CONFIG_GLOBAL=/dev/null", body)
                self.assertIn("GIT_CONFIG_SYSTEM=/dev/null", body)
                self.assertIn("GIT_CONFIG_NOSYSTEM=1", body)
                self.assertIn("GIT_CONFIG_KEY_0=credential.helper", body)
                self.assertIn("GIT_CONFIG_KEY_1=core.hooksPath", body)

    def test_each_replica_rechecks_the_global_claim_before_any_acquisition(self) -> None:
        produce = workflow_job("produce")
        self.assertIn("--verify-dispatch-claim", produce)
        verification = produce.index("--verify-dispatch-claim")
        for effect in (
            "rust-toolchain@",
            "native_shadow_boot_rustdist_acquire_arm64_v1.py",
            "native_shadow_boot_ci_payload_acquire_arm64_v1.py",
            "native_shadow_boot_writer_set_acquire_arm64_v1.py",
            "native_shadow_launcher_emit_arm64_v2.py",
            "RUNNER_TEMP/acquire",
            "sudo /usr/bin/env -i",
            "--replica-ordinal",
        ):
            self.assertLess(verification, produce.index(effect))
        self.assertIn("git fetch", produce[:verification])
        self.assertIn("${{ github.run_id }}", produce[:verification])
        self.assertIn("${{ github.run_attempt }}", produce[:verification])
        self.assertIn("${{ github.workflow_ref }}", produce[:verification])
        self.assertIn("${{ github.sha }}", produce[:verification])

    def test_root_production_step_never_interpolates_github_context_in_shell(self) -> None:
        produce = workflow_job("produce")
        marker = "      - name: Invoke the root-installed production isolation exactly once\n"
        root_step = produce.split(marker, 1)[1]
        following = root_step.find("\n      - name:")
        if following >= 0:
            root_step = root_step[:following]

        for name, context in (
            ("GITHUB_RUN_ID_VALUE", "${{ github.run_id }}"),
            ("GITHUB_RUN_ATTEMPT_VALUE", "${{ github.run_attempt }}"),
            ("GITHUB_REPOSITORY_VALUE", "${{ github.repository }}"),
            ("GITHUB_WORKFLOW_REF_VALUE", "${{ github.workflow_ref }}"),
        ):
            self.assertIn(f"{name}: {context}", root_step)
            self.assertIn(f'"${name}"', root_step)
            run_body = root_step.split("        run: |\n", 1)[1]
            self.assertNotIn(context, run_body)

    def test_guard_created_tag_object_sha_is_handed_to_every_consumer(self) -> None:
        guard = workflow_job("production-authority-guard")
        produce = workflow_job("produce")
        compare = workflow_job("compare")
        self.assertIn("outputs:", guard)
        self.assertIn(
            "tag-object-sha: ${{ steps.create-claim.outputs.tag_object_sha }}",
            guard,
        )
        self.assertIn("id: create-claim", guard)
        handoff = "${{ needs.production-authority-guard.outputs.tag-object-sha }}"
        for body in (produce, compare):
            self.assertIn(handoff, body)
            self.assertIn('--tag-object-sha "$guard_tag_object_sha"', body)
        self.assertIn("needs: [production-authority-guard, produce]", compare)

    def test_qualified_replicas_are_kept_seven_days_and_compared_byte_exact(self) -> None:
        produce = workflow_job("produce")
        compare = workflow_job("compare")
        self.assertEqual(produce.count("upload-artifact@"), 1)
        self.assertIn("retention-days: 7", produce)
        self.assertIn("compression-level: 1", produce)
        self.assertIn("if-no-files-found: error", produce)
        self.assertIn("if: success()", produce)
        self.assertNotIn("if: always()", produce)
        self.assertIn(
            "native-shadow-successor-v5-replica-${{ matrix.replica }}", produce
        )
        upload = produce[produce.index("upload-artifact@") :]
        self.assertIn("path: |", upload)
        self.assertIn(
            "${{ runner.temp }}/successor-v5-replica-${{ matrix.replica }}/outputs",
            upload,
        )
        self.assertIn(
            "${{ runner.temp }}/successor-v5-replica-${{ matrix.replica }}/REPLICA-PROVENANCE.json",
            upload,
        )
        self.assertIn("needs: [production-authority-guard, produce]", compare)
        self.assertRegex(compare, r"(?m)^    timeout-minutes: 30$")
        self.assertIn(
            'runner_collection="$RUNNER_TEMP/successor-v5-replica-${REPLICA_ORDINAL}"',
            produce,
        )
        self.assertEqual(compare.count("download-artifact@"), 2)
        for replica in ("1", "2"):
            self.assertIn(f"native-shadow-successor-v5-replica-{replica}", compare)
            self.assertIn(f"$RUNNER_TEMP/replica-{replica}", compare)
        provenance = compare.index("--compare-provenanced-replicas")
        for name in ("guest-kernel", "guest-initrd", "guest-root-disk"):
            literal = f'cmp -- "$left/outputs/{name}" "$right/outputs/{name}"'
            self.assertEqual(compare.count(literal), 1)
            self.assertLess(provenance, compare.index(literal))
        self.assertNotRegex(compare, r"(?<!provenanced-)--compare-replicas\b")
        self.assertNotIn("UNQUALIFIED-DIAGNOSTIC", compare)
        self.assertNotIn("continue-on-error", produce + compare)
        self.assertNotIn("|| true", produce + compare)

    def test_each_matrix_child_seals_logical_provenance_before_upload(self) -> None:
        produce = workflow_job("produce")
        production = produce.index("--production")
        upload = produce.index("upload-artifact@")
        self.assertLess(production, upload)
        self.assertNotIn("--emit-replica-provenance", produce)
        for fragment in (
            '--replica-ordinal "$replica"',
            '--strategy-job-index "$strategy_job_index"',
            '--strategy-job-total "$strategy_job_total"',
            '--github-job "$github_job"',
            '--artifact-name "native-shadow-successor-v5-replica-$replica"',
        ):
            self.assertIn(fragment, produce[production:upload])
        source = WORKFLOW.read_text(encoding="utf-8")
        for forbidden in (
            "${{ runner.name }}",
            "physicalRunnerIndependenceClaim: true",
            "physically independent",
            "independent runner",
            "independent host",
        ):
            self.assertNotIn(forbidden, source)

    def test_rehearsal_runs_the_v4_bytes_and_keeps_one_canonical_r2_json(self) -> None:
        body = workflow_job("free-rehearsal")
        self.assertIn("runs-on: ubuntu-24.04-arm", body)
        gate = body.index("--verify-bindings-only")
        for repository_python in (
            "native_shadow_boot_rustdist_acquire_arm64_v1.py",
            "native_shadow_boot_ci_payload_acquire_arm64_v1.py",
            "native_shadow_boot_writer_set_acquire_arm64_v1.py",
            "native_shadow_launcher_emit_arm64_v2.py",
        ):
            self.assertLess(gate, body.index(repository_python))
        self.assertIn("--rehearsal-only", body)
        self.assertNotIn("--outputs", body)
        self.assertNotIn("sudo ./scripts/native-shadow-successor-produce", body)
        self.assertIn("R3-RESULT.json", body)
        self.assertIn("member_count", body)
        self.assertIn("json.dumps(d, indent=2, sort_keys=True)", body)
        self.assertEqual(body.count("upload-artifact@"), 1)
        for forbidden in (
            "guest-kernel",
            "guest-initrd",
            "guest-root-disk",
            "ATTEMPT-" + "CONSUMED.json",
            "mke2fs",
            "mkinitramfs",
            "qemu-img",
        ):
            self.assertNotIn(forbidden, body)

    def test_rehearsal_acquires_every_package_before_root_isolation(self) -> None:
        body = workflow_job("free-rehearsal")
        root_isolation = body.index("sudo /usr/bin/env -i")
        for acquirer in (
            "native_shadow_boot_rustdist_acquire_arm64_v1.py",
            "native_shadow_boot_ci_payload_acquire_arm64_v1.py",
            "native_shadow_boot_writer_set_acquire_arm64_v1.py",
        ):
            self.assertLess(body.index(acquirer), root_isolation)
        self.assertEqual(body.count('--cas "$PWD/$CAS"'), 2)

    def test_rehearsal_acquirers_cover_every_low_level_package_digest(self) -> None:
        source_lock_v2 = json.loads(
            (
                REPO
                / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v2.json"
            ).read_text(encoding="utf-8")
        )
        required = {str(row["sha256"]) for row in source_lock_v2["artifacts"]}
        pins = writer_tree.sealed_pins()
        required.add(str(pins["writer"]["packageSha256"]))
        required.update(str(row["packageSha256"]) for row in pins["libraries"])

        rust_plan = rustdist.load_plan()
        payload_plan = ci_payload.derive_plan(REPO)
        writer_plan = writer_set.derive_plan()
        acquired = {str(row["sha256"]) for row in rust_plan["artifacts"]}
        acquired.update(str(row["sha256"]) for row in payload_plan["artifacts"])
        acquired.update(
            str(row["sha256"]) for row in payload_plan["derivedArtifacts"]
        )
        acquired.update(str(row["sha256"]) for row in writer_plan["artifacts"])
        self.assertEqual(acquired, required)

        documents, _ = ci_payload._sealed(REPO)
        source_lock_v1 = {
            str(row["id"]): str(row["sha256"])
            for row in documents["bootSourceLock"]["artifacts"]
        }
        reused = {
            source_lock_v1[str(identifier)]
            for identifier in payload_plan["reusedArtifactIds"]
        }
        self.assertEqual(
            reused,
            {str(row["sha256"]) for row in rust_plan["artifacts"]},
        )
        self.assertEqual(
            {rustdist.CAS_ROOT, ci_payload.CAS_ROOT, writer_set.CAS_ROOT},
            {ci_payload.CAS_ROOT},
        )

    def test_rehearsal_collection_has_exactly_one_plain_single_link_member(self) -> None:
        body = workflow_job("free-rehearsal")
        collection = body[body.index("Require exactly one canonical R3 JSON member") :]
        self.assertIn(
            'find "$RUNNER_TEMP/rehearsal-artifact" -mindepth 1 -maxdepth 1 '
            '| wc -l',
            collection,
        )
        self.assertNotIn("-maxdepth 1 -type f | wc -l", collection)
        self.assertIn('test -f "$artifact"', collection)
        self.assertIn('test ! -L "$artifact"', collection)
        self.assertIn('test "$(stat -c %h -- "$artifact")" = 1', collection)


class RootExecutionAnchorContractTests(unittest.TestCase):
    def test_rehearsal_completes_history_without_importing_tags_for_bundle_fsck(self) -> None:
        rehearsal = workflow_job("free-rehearsal")
        checkout = rehearsal[
            rehearsal.index("actions/checkout@") : rehearsal.index(
                "Verify the complete preregistered v5 bindings first"
            )
        ]
        self.assertIn("fetch-depth: 1", checkout)
        self.assertIn("fetch-tags: false", checkout)
        self.assertIn("persist-credentials: false", checkout)
        completion = rehearsal.index("git fetch --no-tags --unshallow")
        bundle = rehearsal.index('bundle create "$runner_bundle" HEAD')
        self.assertLess(completion, bundle)
        history = rehearsal[checkout.index("fetch-depth: 1") : bundle]
        self.assertIn("refs/tags/", history)
        self.assertIn("--is-shallow-repository", history)

    def test_production_verifies_claim_then_completes_history_without_other_tags(self) -> None:
        produce = workflow_job("produce")
        checkout = produce[
            produce.index("actions/checkout@") : produce.index(
                "Recheck authority before this replica acquires anything"
            )
        ]
        self.assertIn("fetch-depth: 1", checkout)
        self.assertIn("fetch-tags: false", checkout)
        self.assertIn("persist-credentials: false", checkout)
        verification = produce.index("--verify-dispatch-claim")
        completion = produce.index("git fetch --no-tags --unshallow")
        bundle = produce.index('bundle create "$runner_bundle"')
        self.assertLess(verification, completion)
        self.assertLess(completion, bundle)
        self.assertIn(
            '[[ $(git for-each-ref --format=\'%(refname)\' refs/tags/) == "$CLAIM_REF" ]]',
            produce[verification:bundle],
        )

    def test_compare_checkout_cannot_preimport_the_create_once_claim(self) -> None:
        compare = workflow_job("compare")
        checkout = compare[
            compare.index("actions/checkout@") : compare.index(
                "Fetch and verify the exact create-once production claim"
            )
        ]
        self.assertIn("fetch-depth: 1", checkout)
        self.assertIn("fetch-tags: false", checkout)
        self.assertIn("persist-credentials: false", checkout)

    def test_rehearsal_root_edge_executes_only_one_verified_head_anchor(self) -> None:
        rehearsal = workflow_job("free-rehearsal")
        for required in (
            'bundle create "$runner_bundle" HEAD',
            'readonly anchor="$anchor_parent/${head_sha}-rehearsal"',
            'readonly anchor_partial="${anchor}.partial"',
            'bundle verify "$root_bundle"',
            "/usr/bin/git clone",
            "fsck --strict --full",
            "HEAD^{commit}",
            "/usr/bin/mv -T",
            "/usr/bin/sync -f",
            "-type l",
            "-perm /0022",
            'readonly anchored_wrapper="$anchor/repo/scripts/native-shadow-successor-produce-arm64-v5.sh"',
            '"$anchored_wrapper" --rehearsal-only',
        ):
            self.assertIn(required, rehearsal)
        self.assertEqual(rehearsal.count("sudo /usr/bin/env -i"), 1)
        self.assertNotIn(
            "sudo ./scripts/native-shadow-successor-produce-arm64-v5.sh",
            rehearsal,
        )

    def test_rehearsal_rechecks_every_root_directory_created_for_the_anchor(self) -> None:
        rehearsal = workflow_job("free-rehearsal")
        created = rehearsal.index(
            "/usr/bin/install -d -o root -g root -m 0755"
        )
        partial = rehearsal.index("[[ ! -e $anchor && ! -L $anchor ]]", created)
        recheck = rehearsal[created:partial]
        self.assertIn(
            'for candidate in /var /var/lib /var/lib/boole "$base"', recheck
        )
        self.assertIn('"$anchor_parent" "$rehearsal_root"; do', recheck)
        self.assertIn('[[ -d $candidate && ! -L $candidate ]]', recheck)
        self.assertIn(
            '[[ $(/usr/bin/readlink -f -- "$candidate") == "$candidate" ]]',
            recheck,
        )
        self.assertIn(
            '[[ $(/usr/bin/stat -c %u:%g -- "$candidate") == 0:0 ]]',
            recheck,
        )
        self.assertIn('[[ $((8#$candidate_mode & 8#22)) -eq 0 ]]', recheck)

    def test_root_edge_never_executes_checkout_shell_or_python(self) -> None:
        produce = workflow_job("produce")
        for forbidden in (
            "sudo ./scripts/native-shadow-successor-produce-arm64-v5.sh",
            'parent="$RUNNER_TEMP/successor-v5-replica-',
            'sudo /usr/bin/python3 -I -S "$GITHUB_WORKSPACE/',
            'sudo python3 ',
        ):
            self.assertNotIn(forbidden, produce)
        self.assertIn(
            "readonly base=/var/lib/boole/native-shadow-successor-v5", produce
        )
        self.assertIn('readonly anchor_parent="$base/anchors"', produce)
        self.assertIn('bundle create "$runner_bundle"', produce)
        self.assertIn("/usr/bin/git clone", produce)
        self.assertIn("sudo /usr/bin/env -i", produce)
        self.assertIn('"$anchored_wrapper" --production', produce)

    def test_root_anchor_is_exact_atomic_and_nonwritable(self) -> None:
        produce = workflow_job("produce")
        for required in (
            'bundle verify "$root_bundle"',
            'fsck --strict --full',
            "HEAD^{commit}",
            "refs/tags/boole-native-shadow-mac3-successor-production-a7-",
            'readonly anchor_partial="${anchor}.partial"',
            "/usr/bin/mv -T",
            "/usr/bin/sync -f",
            "/usr/bin/chown -R 0:0",
            "/usr/bin/find",
            "-type l",
            "-perm /0022",
            "120000",
            "160000",
            "objects/info/alternates",
            "diff-index --quiet",
            "ls-files --others",
            "stat -c %h",
        ):
            self.assertIn(required, produce)
        self.assertLess(produce.index('bundle verify "$root_bundle"'), produce.index("/usr/bin/git clone"))
        self.assertLess(produce.index("/usr/bin/git clone"), produce.index("/usr/bin/mv -T"))
        self.assertLess(produce.index("/usr/bin/mv -T"), produce.index('"$anchored_wrapper" --production'))

    def test_root_wrapper_refuses_every_non_anchor_execution_root(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("require_root_execution_anchor()", source)
        validator = source[
            source.index("require_root_execution_anchor()") :
            source.index("verify_preregistered_bindings()")
        ]
        self.assertIn(
            "/var/lib/boole/native-shadow-successor-v5/anchors/", validator
        )
        self.assertIn('[[ $(stat -c %u:%g -- "$candidate") == "0:0" ]]', validator)
        self.assertIn('[[ $((8#$permission_mode & 8#22)) -eq 0 ]]', validator)
        self.assertIn('[[ -d $ROOT/.git && ! -L $ROOT/.git ]]', validator)
        binding_call = source.index("\nverify_preregistered_bindings\n")
        root_gate = source.rindex('if [[ ${EUID} -eq 0 ]]', 0, binding_call)
        self.assertLess(root_gate, binding_call)
        self.assertIn(
            "require_root_execution_anchor", source[root_gate:binding_call]
        )

    def test_root_git_reader_disables_replace_objects(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        git_reader = source[source.index("git_repo()") : source.index("prepare_dispatch_context()")]
        self.assertIn("--no-replace-objects", git_reader)
        self.assertIn("GIT_OPTIONAL_LOCKS=0", git_reader)

    def test_production_export_has_only_root_owned_nonwritable_ancestry(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("require_claim_bound_export_parent()", source)
        validator = source[
            source.index("require_claim_bound_export_parent()") :
            source.index("require_inputs()")
        ]
        self.assertIn(
            "/var/lib/boole/native-shadow-successor-v5/exports/", validator
        )
        self.assertIn('expected_tag_object_sha', validator)
        self.assertIn('replica_ordinal', validator)
        self.assertIn('stat -c %u:%g', validator)
        self.assertIn('$((8#$mode & 8#22))', validator)
        production = source[source.index('if [[ $mode == "produce" ]]') :]
        production_parent = production.index('outputs_parent="$(dirname -- "$outputs")"')
        production_guard = production.index(
            'require_claim_bound_export_parent "$outputs_parent"'
        )
        production_inputs = production.index("require_inputs allow-missing-result-parent")
        self.assertLess(production_parent, production_guard)
        self.assertLess(production_guard, production_inputs)

        cleanup = source[source.index('if [[ $mode == "cleanup-only" ]]') :]
        cleanup_parent = cleanup.index('outputs_parent="$(dirname -- "$outputs")"')
        cleanup_guard = cleanup.index(
            'require_claim_bound_export_parent "$outputs_parent"'
        )
        cleanup_recovery = cleanup.index('require_recovery_output_parent "$outputs_parent"')
        self.assertLess(cleanup_parent, cleanup_guard)
        self.assertLess(cleanup_guard, cleanup_recovery)

        produce = workflow_job("produce")
        self.assertIn('readonly export_root="$base/exports"', produce)
        self.assertIn('readonly export_tag="$export_root/$tag_object_sha"', produce)
        self.assertNotIn(
            '$RUNNER_TEMP/successor-v4-replica-${{ matrix.replica }}', produce
        )


class WrapperSurfaceTests(unittest.TestCase):
    def test_wrapper_ignores_a_caller_supplied_shadow_tool_path(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            fake = pathlib.Path(temporary)
            marker = fake / "shadow-ran"
            python = fake / "python3"
            python.write_text(
                "#!/bin/sh\nprintf shadow > \"$BOOLE_SHADOW_MARKER\"\nexit 97\n",
                encoding="utf-8",
            )
            python.chmod(0o755)
            environment = os.environ.copy()
            environment["PATH"] = f"{fake}:/usr/sbin:/usr/bin:/sbin:/bin"
            environment["BOOLE_SHADOW_MARKER"] = str(marker)

            completed = subprocess.run(
                ["/bin/bash", str(WRAPPER), "--verify-bindings-only"],
                cwd=REPO,
                env=environment,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )

        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertFalse(marker.exists())

    def test_isolated_python_processes_receive_only_the_fixed_environment(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertNotIn("/usr/bin/env python3", source)
        self.assertGreaterEqual(source.count("/usr/bin/env -i"), 5)
        self.assertGreaterEqual(source.count("/usr/bin/python3 -I -S"), 4)
        for inherited in (
            "GIT_DIR",
            "GIT_WORK_TREE",
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "PYTHONPATH",
            "PYTHONHOME",
        ):
            self.assertIn(inherited, source)

    def test_dispatch_claim_ignores_foreign_cwd_and_injected_git_dir(self) -> None:
        head_sha = subprocess.run(
            ["/usr/bin/git", "-C", str(REPO), "rev-parse", "HEAD"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
        ).stdout.strip()
        authority_relative = (
            "native/containment/"
            "native-shadow-mac3-successor-production-authority-arm64-v7.json"
        )
        authority_blob = subprocess.run(
            [
                "/usr/bin/git",
                "-C",
                str(REPO),
                "cat-file",
                "blob",
                f"{head_sha}:{authority_relative}",
            ],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        a7_in_head = authority_blob.returncode == 0
        authority_sha256 = (
            hashlib.sha256(authority_blob.stdout).hexdigest()
            if a7_in_head
            else "0" * 64
        )
        with tempfile.TemporaryDirectory() as temporary:
            foreign = pathlib.Path(temporary) / "foreign"
            foreign.mkdir()
            subprocess.run(
                ["/usr/bin/git", "init", "-q", str(foreign)], check=True
            )
            subprocess.run(
                [
                    "/usr/bin/git",
                    "-C",
                    str(foreign),
                    "-c",
                    "user.name=Foreign",
                    "-c",
                    "user.email=foreign@example.invalid",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "foreign",
                ],
                check=True,
            )
            environment = os.environ.copy()
            environment["GIT_DIR"] = str(foreign / ".git")
            environment["GIT_WORK_TREE"] = str(foreign)
            environment["GIT_CONFIG_COUNT"] = "1"
            environment.pop("GIT_CONFIG_KEY_0", None)
            trace_marker = pathlib.Path(temporary) / "injected-git-trace"
            environment["GIT_TRACE"] = str(trace_marker)
            completed = subprocess.run(
                [
                    "/bin/bash",
                    str(WRAPPER),
                    "--dispatch-claim-message",
                    "--github-run-id",
                    "33299900001",
                    "--github-run-attempt",
                    "1",
                    "--event-name",
                    EXACT_EVENT_NAME,
                    "--dispatch-ref",
                    EXACT_DISPATCH_REF,
                    "--workflow-ref",
                    EXACT_WORKFLOW_REF,
                    "--workflow-path",
                    WORKFLOW.relative_to(REPO).as_posix(),
                    "--head-authority-sha256",
                    authority_sha256,
                    "--head-sha",
                    head_sha,
                ],
                cwd=foreign,
                env=environment,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertFalse(
                trace_marker.exists(),
                "the wrapper leaked the caller's Git trace environment",
            )

        self.assertNotIn("the checkout HEAD differs", completed.stderr)
        if a7_in_head:
            self.assertEqual(completed.returncode, 0, completed.stderr)
            document = json.loads(completed.stdout)
            self.assertEqual(document["headSha"], head_sha)
            self.assertEqual(
                document["workflowPath"], WORKFLOW.relative_to(REPO).as_posix()
            )
        else:
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn("the HEAD authority blob cannot be hashed", completed.stderr)

    def test_wrapper_is_strict_executable_and_exposes_effect_scoped_modes(self) -> None:
        info = WRAPPER.lstat()
        self.assertTrue(stat.S_ISREG(info.st_mode))
        self.assertFalse(WRAPPER.is_symlink())
        self.assertTrue(info.st_mode & stat.S_IXUSR)
        syntax = subprocess.run(
            ["bash", "-n", str(WRAPPER)],
            cwd=REPO,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(syntax.returncode, 0, syntax.stderr)
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("set -euo pipefail", source)
        for mode in (
            "--verify-bindings-only",
            "--rehearsal-only",
            "--verify-production-authority-only",
            "--dispatch-claim-message",
            "--verify-dispatch-claim",
            "--compare-provenanced-replicas",
            "--preflight-only",
            "--production",
        ):
            self.assertIn(mode, source)
        self.assertNotIn("--emit-replica-provenance", source)

    def test_dispatch_claim_adapters_finish_before_host_or_output_work(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn('mode="dispatch-claim-message"', source)
        self.assertIn('mode="dispatch-claim-verify"', source)
        self.assertIn('--github-run-id "$github_run_id"', source)
        self.assertIn('--github-run-attempt "$github_run_attempt"', source)
        self.assertIn('--event-name "$event_name"', source)
        self.assertIn('--dispatch-ref "$dispatch_ref"', source)
        self.assertIn('--workflow-ref "$workflow_ref"', source)
        self.assertIn('--workflow-path "$workflow_path"', source)
        self.assertIn('--head-sha "$head_sha"', source)
        self.assertIn(
            '--head-authority-sha256 "$head_authority_sha256"', source
        )
        self.assertIn('--claim-ref "$claim_ref"', source)
        self.assertIn('--ref-object-sha "$ref_object_sha"', source)
        self.assertIn('--tag-object-sha "$tag_object_sha"', source)
        plan = source.index('if [[ $mode == "dispatch-claim-message" ]]')
        verify = source.index('if [[ $mode == "dispatch-claim-verify" ]]')
        host = source.index("require_host()")
        self.assertLess(plan, host)
        self.assertLess(verify, host)
        comparison = source.index(
            'if [[ $mode == "compare-provenanced-replicas" ]]'
        )
        for block_start, block_end, core_mode in (
            (plan, verify, "dispatch-claim-message"),
            (verify, comparison, "dispatch-claim-verify"),
        ):
            block = source[block_start:block_end]
            self.assertIn(f'"$PRODUCER" {core_mode}', block)
            for forbidden in ("mktemp", "mount ", "--outputs", "--cas", "--launcher"):
                self.assertNotIn(forbidden, block)

    def test_replica_envelope_adapters_reverify_the_live_tag_before_host_work(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        comparison = source.index(
            'if [[ $mode == "compare-provenanced-replicas" ]]'
        )
        host = source.index("require_host()")
        self.assertLess(comparison, host)
        blocks = ((
            source[comparison:host],
            "compare-provenanced-replicas",
            ("--left-bundle", "--right-bundle"),
        ),)
        for block, core_mode, expected in blocks:
            self.assertIn(f'"$PRODUCER" {core_mode}', block)
            self.assertIn("resolve_dispatch_claim", block)
            self.assertIn("recheck_dispatch_claim_ref", block)
            for argument in expected:
                self.assertIn(argument, block)
            for forbidden in (
                "mktemp",
                "mount ",
                "systemd-run",
                "--cas",
                "--launcher",
                "mke2fs",
                "mkinitramfs",
            ):
                self.assertNotIn(forbidden, block)
        self.assertNotIn('mode="compare-replicas"', source)

    def test_production_rechecks_the_guard_tag_before_preflight_and_marker(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        production_gate = source.index(
            'if [[ $mode == "preflight" || $mode == "produce" ]]'
        )
        scratch = source.index('scratch="$(mktemp -d', production_gate)
        preflight = source.index('if [[ $mode == "produce" ]]; then', scratch)
        preflight_removed = source.index(
            'rm -rf -- "$staging_preflight"', preflight
        )
        production_core = source.index('"$PRODUCER" produce', preflight_removed)

        first = source.index("verify_live_dispatch_claim", production_gate)
        second = source.index("snapshot_and_verify_dispatch_claim", preflight_removed)
        self.assertLess(first, scratch)
        self.assertLess(preflight_removed, second)
        self.assertLess(second, production_core)
        invocation = source[
            second : source.index("qualification_argv=()", second)
        ]
        self.assertIn('--tag-object-sha "$expected_tag_object_sha"', invocation)
        self.assertIn('--claim-ref "$claim_ref"', invocation)
        self.assertIn('< "$dispatch_tag_snapshot"', invocation)

    def test_replica_envelope_modes_reject_cross_scope_arguments_before_effects(
        self,
    ) -> None:
        cases = (
            (
                ["--verify-bindings-only", "--replica-ordinal", "1"],
                "only production accepts replica metadata",
            ),
            (
                ["--compare-provenanced-replicas", "--outputs", "/tmp/no"],
                "provenanced comparison accepts no production output input",
            ),
            (
                ["--verify-bindings-only", "--left-bundle", "/tmp/no"],
                "only the provenanced comparison accepts bundle input",
            ),
        )
        for arguments, message in cases:
            with self.subTest(arguments=arguments):
                if arguments[0] == "--compare-provenanced-replicas":
                    arguments = [
                        *arguments,
                        "--github-run-id",
                        "1",
                        "--github-run-attempt",
                        "1",
                        "--event-name",
                        EXACT_EVENT_NAME,
                        "--dispatch-ref",
                        EXACT_DISPATCH_REF,
                        "--workflow-path",
                        WORKFLOW.relative_to(REPO).as_posix(),
                        "--workflow-ref",
                        EXACT_WORKFLOW_REF,
                        "--head-sha",
                        "0" * 40,
                        "--head-authority-sha256",
                        "0" * 64,
                    ]
                completed = subprocess.run(
                    ["bash", str(WRAPPER), *arguments],
                    cwd=REPO,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                )
                self.assertNotEqual(completed.returncode, 0)
                self.assertIn(message, completed.stderr)

    def test_dispatch_claim_uses_live_git_facts_without_normalising_them(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        adapter = source[
            source.index("git_repo()") : source.index("prepare_dispatch_context()")
        ]
        self.assertIn("/usr/bin/env -i", adapter)
        self.assertIn("/usr/bin/git", adapter)
        self.assertIn('-C "$ROOT"', adapter)
        self.assertIn('safe.directory=$ROOT', adapter)
        self.assertIn("GIT_CONFIG_GLOBAL=/dev/null", adapter)
        self.assertIn("GIT_CONFIG_NOSYSTEM=1", adapter)
        without_adapter = source[: source.index("git_repo()")] + source[
            source.index("prepare_dispatch_context()") :
        ]
        self.assertNotRegex(
            without_adapter,
            r"(?<![A-Za-z0-9_/])(?:/usr/bin/)?git[ \t]+"
            r"(?:rev-parse|cat-file|check-ref-format|show-ref)\b",
        )
        self.assertIn('git_repo rev-parse --verify "HEAD^{commit}"', source)
        self.assertIn(
            'git_repo cat-file blob "$head_sha:$authority_relative"', source
        )
        self.assertIn('git_repo check-ref-format "$claim_ref"', source)
        self.assertNotIn("check-ref-format --normalize", source)
        self.assertIn('[[ $github_run_attempt == "1" ]]', source)
        self.assertIn('[[ $event_name == "$exact_event_name" ]]', source)
        self.assertIn('[[ $dispatch_ref == "$exact_dispatch_ref" ]]', source)
        self.assertIn('[[ $workflow_path == "$v5_workflow_path" ]]', source)
        self.assertIn('[[ $workflow_ref == "$exact_workflow_ref" ]]', source)
        self.assertNotIn("workflow_prefix=", source)
        self.assertIn('git_repo rev-parse --verify "$claim_ref^{tag}"', source)
        self.assertIn('git_repo cat-file -t "$tag_object_sha"', source)
        self.assertIn('git_repo cat-file tag "$tag_object_sha"', source)
        for forbidden in (
            "git tag -f",
            "update-ref -d",
            "check-ref-format --normalize",
            "strip()",
            ".lower()",
        ):
            self.assertNotIn(forbidden, source)

    def test_dispatch_attempt_id_is_parsed_from_the_same_head_blob(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        start = source.index("prepare_dispatch_context()")
        end = source.index("resolve_dispatch_claim()", start)
        block = source[start:end]
        head_blob = 'git_repo cat-file blob "$head_sha:$authority_relative"'
        self.assertEqual(block.count(head_blob), 2)
        self.assertIn("sys.stdin.buffer.read(1048577)", block)
        self.assertNotIn('"$ROOT/$authority_relative"', block)
        self.assertIn('grant = document.get("grant")', block)
        self.assertIn('attempt_id = grant.get("attemptId")', block)
        self.assertIn('if not isinstance(grant, dict):', block)
        self.assertNotIn('document.get("attemptId")', block)

    def test_binding_only_mode_rechecks_p2_without_requiring_future_r2(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            scratch = pathlib.Path(directory)
            forbidden_output = scratch / "wrapper-output"
            completed = subprocess.run(
                ["bash", str(WRAPPER), "--verify-bindings-only"],
                cwd=REPO,
                env={**os.environ, "TMPDIR": str(scratch)},
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertIn("bindings verified", completed.stderr.lower())
            self.assertFalse(forbidden_output.exists())
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn(
            "63f5bdf0ffaac00ac1af3972ed69051da9fcbe8a06b90ae3c9f70756bbfe144b",
            source,
        )
        self.assertIn("13335", source)
        self.assertIn("os.path.lexists", source)

    def test_binding_only_mode_rejects_missing_or_tampered_p3_before_import(self) -> None:
        p4_relative = (
            "native/containment/native-shadow-mac3-launcher-v2-successor-main-"
            "branch-dispatch-fence-correction-arm64-v1.json"
        )
        p4 = json.loads((REPO / p4_relative).read_text(encoding="utf-8"))
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            wrapper = root / "scripts/native-shadow-successor-produce-arm64-v5.sh"
            wrapper.parent.mkdir(parents=True)
            wrapper.write_bytes(WRAPPER.read_bytes())
            wrapper.chmod(0o755)
            for relative in (
                p4_relative,
                *(row["path"] for row in p4["predecessors"]),
            ):
                destination = root / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes((REPO / relative).read_bytes())

            p4_copy = root / p4_relative
            p4_copy.unlink()
            missing = subprocess.run(
                ["bash", str(wrapper), "--verify-bindings-only"],
                cwd=root,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertNotEqual(missing.returncode, 0)
            self.assertIn("dispatch-fence-correction", missing.stderr)

            p4_copy.write_bytes((REPO / p4_relative).read_bytes() + b"\n")
            tampered = subprocess.run(
                ["bash", str(wrapper), "--verify-bindings-only"],
                cwd=root,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertNotEqual(tampered.returncode, 0)
            self.assertIn("binding size differs", tampered.stderr)

    def test_authority_only_mode_refuses_before_host_or_output_work(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            scratch = pathlib.Path(directory)
            completed = subprocess.run(
                ["bash", str(WRAPPER), "--verify-production-authority-only"],
                cwd=REPO,
                env={**os.environ, "TMPDIR": str(scratch)},
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                check=False,
            )
            self.assertNotEqual(completed.returncode, 0)
            self.assertIn(
                "production-check lacks github_run_id", completed.stderr.lower()
            )
            self.assertFalse((scratch / "boot-outputs").exists())
        source = WRAPPER.read_text(encoding="utf-8")
        authority = source.index('"$PRODUCER" production-check')
        for later in ("command -v", "uname -s", "mktemp -d"):
            self.assertLess(authority, source.index(later, authority))

    def test_effect_free_modes_reject_production_arguments_before_host_work(self) -> None:
        cases = (
            (["--verify-bindings-only", "--outputs", "/tmp/no"], "accepts no other input"),
            (["--verify-production-authority-only", "--outputs", "/tmp/no"], "accepts no other input"),
            (["--dispatch-claim-message", "--outputs", "/tmp/no"], "accepts no image or output"),
            (["--verify-dispatch-claim", "--cas", "/tmp/no"], "accepts no image or output"),
            (["--rehearsal-only", "--outputs", "/tmp/no"], "accepts no --outputs"),
            (["--preflight-only", "--outputs", "/tmp/no"], "accepts no --outputs"),
        )
        for arguments, message in cases:
            with self.subTest(arguments=arguments):
                completed = subprocess.run(
                    ["bash", str(WRAPPER), *arguments],
                    cwd=REPO,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                )
                self.assertNotEqual(completed.returncode, 0)
                self.assertIn(message, completed.stderr)

    def test_rehearsal_and_preflight_call_only_the_v4_core_without_outputs(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn('"$PRODUCER" rehearsal', source)
        self.assertIn('"$PRODUCER" preflight', source)
        self.assertIn('--repository-root "$ROOT"', source)
        self.assertIn('--cas "$cas"', source)
        self.assertIn('--launcher "$launcher"', source)
        self.assertIn('--scratch "$', source)
        self.assertIn('--result "$result"', source)
        rehearsal = source[
            source.index('if [[ $mode == "rehearsal" ]]; then') :
            source.index(
                "# Both production preflight",
                source.index('if [[ $mode == "rehearsal" ]]; then'),
            )
        ]
        preflight_start = source.index('if [[ $mode == "preflight" ]]; then')
        preflight = source[
            preflight_start : source.index('if [[ $mode == "produce" ]]; then', preflight_start)
        ]
        for block in (rehearsal, preflight):
            self.assertNotIn("--outputs", block)
            self.assertNotIn("ATTEMPT-" + "CONSUMED.json", block)

    def test_rehearsal_uses_the_same_bounded_transient_isolation(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        start = source.index('if [[ $mode == "rehearsal" ]]; then')
        rehearsal = source[start : source.index("# Both production preflight", start)]
        self.assertIn('[[ ${EUID} -eq 0 ]]', rehearsal)
        self.assertIn('require_dedicated_write_parent "$result_parent"', rehearsal)
        self.assertIn("mount -t tmpfs", rehearsal)
        self.assertIn(
            'size=${staging_tmpfs_size_bytes},nr_inodes=${staging_tmpfs_inodes}',
            rehearsal,
        )
        self.assertIn("while IFS= read -r item", rehearsal)
        self.assertIn('< <(isolation_prefix "$rehearsal_unit")', rehearsal)
        self.assertIn('"--property=ReadWritePaths=$rehearsal_scratch"', rehearsal)
        self.assertIn('"--property=ReadWritePaths=$result_parent"', rehearsal)
        self.assertIn('"${rehearsal_argv[@]}"', rehearsal)
        self.assertNotIn("publish_collectable_parent", rehearsal)

    def test_rehearsal_opens_only_one_verified_result_for_collection(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        start = source.index('if [[ $mode == "rehearsal" ]]; then')
        rehearsal = source[start : source.index("# Both production preflight", start)]
        identity = rehearsal.index(
            'rehearsal_parent_identity="$(stat -c %d:%i -- "$result_parent")"'
        )
        call = rehearsal.index('"${rehearsal_argv[@]}"', identity)
        recheck = rehearsal.index(
            '[[ $(stat -c %d:%i -- "$result_parent") == "$rehearsal_parent_identity" ]]',
            call,
        )
        exact_result = rehearsal.index(
            'find "$result_parent" -mindepth 1 -maxdepth 1 '
            '! -path "$result" -print -quit',
            call,
        )
        open_parent = rehearsal.index('chmod 0711 -- "$result_parent"', exact_result)
        self.assertLess(identity, call)
        self.assertLess(call, recheck)
        self.assertLess(recheck, exact_result)
        self.assertLess(exact_result, open_parent)
        self.assertIn('[[ -f $result && ! -L $result ]]', rehearsal)
        self.assertIn('[[ $(stat -c %u:%g -- "$result_parent") == "0:0" ]]', rehearsal)
        self.assertIn('[[ $(stat -c %a -- "$result_parent") == "700" ]]', rehearsal)
        self.assertIn('[[ $(stat -c %a -- "$result") == "444" ]]', rehearsal)

    def test_wrapper_and_workflow_never_call_a_historical_generation(self) -> None:
        combined = WRAPPER.read_text(encoding="utf-8") + WORKFLOW.read_text(
            encoding="utf-8"
        )
        forbidden = (
            "scripts/native-shadow-successor-produce-arm64-v3.sh",
            ".github/workflows/native-shadow-successor-produce-arm64-v3.yml",
            "scripts/native-shadow-successor-produce-arm64.sh",
            ".github/workflows/native-shadow-successor-produce-arm64.yml",
            "scripts/native_shadow_successor_produce_phase_arm64_v3.py",
            "scripts/native_shadow_successor_produce_phase_arm64_v2.py",
            "scripts/native_shadow_successor_root_disk_readback_arm64_v2.py",
            "scripts/native_shadow_boot_root_disk_readback_arm64_v1.py",
            "scripts/native_shadow_launcher_emit_arm64_v1.py",
            "native-shadow-launcher-build-result-arm64-v1.json",
            "SUCCESSOR-ROOT-DISK-READBACK.json",
        )
        for path in forbidden:
            with self.subTest(path=path):
                self.assertNotIn(path, combined)
        self.assertIn("scripts/native-shadow-successor-produce-arm64-v5.sh", combined)
        core = (REPO / "scripts/native_shadow_successor_produce_phase_arm64_v5.py").read_text(
            encoding="utf-8"
        )
        self.assertIn("scripts.native_shadow_successor_root_disk_readback_arm64_v3", core)
        self.assertNotIn("native_shadow_successor_root_disk_readback_arm64_v3.py", combined)

    def test_readback_v3_has_one_owner_in_the_core_production_edge(self) -> None:
        wrapper = WRAPPER.read_text(encoding="utf-8")
        workflow = WORKFLOW.read_text(encoding="utf-8")
        core = (REPO / "scripts/native_shadow_successor_produce_phase_arm64_v5.py").read_text(
            encoding="utf-8"
        )
        self.assertNotIn("$READBACK", wrapper)
        self.assertNotIn("root_disk_readback_arm64_v3.py", wrapper + workflow)
        self.assertEqual(
            core.count("scripts.native_shadow_successor_root_disk_readback_arm64_v3"),
            1,
        )
        production = core[
            core.index("def produce(") : core.index("def qualify(")
        ]
        qualification = core[
            core.index("def qualify(") : core.index("class RepositoryImageBackend")
        ]
        self.assertIn("with _consumed_attempt(request)", production)
        self.assertNotIn(".readback(", production)
        self.assertIn("selected.readback(root, output_root, chain)", qualification)
        self.assertIn(
            "with _pinned_outputs(output_root, QUALIFICATION_PINNED_NAMES) as pinned",
            qualification,
        )
        self.assertIn('pinned_root_disk = pinned["guest-root-disk"]', qualification)
        self.assertIn("root_disk_identity = FileIdentity(", qualification)
        self.assertIn("_assert_pinned_outputs_unchanged", qualification)
        self.assertIn(
            "_assert_readback_pass(readback, root_disk_identity, expected_entry_count)",
            qualification,
        )

    def test_production_rechecks_authority_then_preflights_and_calls_core_once(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        common_guard = source.index(
            'if [[ $mode == "preflight" || $mode == "produce" ]]'
        )
        authority = source.index('"$PRODUCER" production-check', common_guard)
        scratch = source.index("mktemp -d", authority)
        production = source.index('if [[ $mode == "produce" ]]; then', scratch)
        preflight = source.index('"$PRODUCER" preflight', production)
        produce = source.index('"$PRODUCER" produce', production)
        qualify = source.index('"$PRODUCER" qualify', production)
        self.assertLess(authority, scratch)
        self.assertLess(scratch, preflight)
        self.assertLess(preflight, produce)
        self.assertLess(produce, qualify)
        self.assertEqual(source.count('"$PRODUCER" produce'), 1)
        self.assertEqual(source.count('"$PRODUCER" qualify'), 1)
        self.assertIn('--pending "$outputs/PRODUCE-RESULT-PENDING-READBACK-V5.json"', source)
        self.assertIn('--result "$result"', source[source.index('"$PRODUCER" qualify') :])
        isolated = source[
            source.index("production_argv=()") : source.index('"${production_argv[@]}"')
        ]
        self.assertIn('"$PRODUCER" produce', isolated)
        self.assertNotIn('"$PRODUCER" qualify', isolated)
        self.assertNotIn('mkdir -p "$outputs"', source)
        self.assertIn("The core owns output-directory creation", source)
        seal = source.index('"$PRODUCER" publish-and-seal-replica', qualify)
        disarmed = source.index('collectability_armed="no"', seal)
        self.assertLess(qualify, seal)
        self.assertLess(seal, disarmed)
        for argument in (
            '--replica-ordinal "$replica_ordinal"',
            '--strategy-job-index "$strategy_job_index"',
            '--strategy-job-total "$strategy_job_total"',
            '--github-job "$github_job"',
            '--artifact-name "$artifact_name"',
            '--parent-device "$parent_device"',
            '--parent-inode "$parent_inode"',
        ):
            self.assertIn(argument, source[seal:disarmed])
        self.assertNotIn("$READBACK", source)

    def test_preflight_and_production_use_fresh_siblings_on_one_tmpfs(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn('staging_preflight="$staging/preflight"', source)
        self.assertIn('staging_production="$staging/production"', source)
        self.assertIn('mkdir -m 0700 "$staging_preflight" "$staging_production"', source)
        self.assertIn('--scratch "$staging_preflight"', source)
        self.assertIn('--scratch "$staging_production"', source)
        self.assertNotIn('--scratch "$scratch/preflight"', source)
        self.assertNotIn('--scratch "$staging"', source)
        mount = source.index('mount -t tmpfs')
        children = source.index('mkdir -m 0700 "$staging_preflight" "$staging_production"')
        preflight = source.index('--scratch "$staging_preflight"', children)
        production = source.index('--scratch "$staging_production"', preflight)
        self.assertLess(mount, children)
        self.assertLess(children, preflight)
        self.assertLess(preflight, production)

    def test_successful_preflight_tree_is_removed_before_production_starts(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        production = source[source.index('if [[ $mode == "produce" ]]; then') :]
        preflight_call = production.index('"${preflight_argv[@]}"')
        removal = production.index('rm -rf -- "$staging_preflight"', preflight_call)
        absent_check = production.index(
            '[[ ! -e $staging_preflight && ! -L $staging_preflight ]]', removal
        )
        production_call = production.index('"${production_argv[@]}"', absent_check)
        self.assertLess(preflight_call, removal)
        self.assertLess(removal, absent_check)
        self.assertLess(absent_check, production_call)

    def test_dispatch_snapshot_is_a_sibling_of_an_exactly_empty_production_scratch(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        production = source[source.index('if [[ $mode == "produce" ]]; then') :]
        removal = production.index('rm -rf -- "$staging_preflight"')
        empty_check = production.index(
            'first_production_member="$(find "$staging_production"',
            removal,
        )
        snapshot = production.index(
            'dispatch_tag_snapshot="$staging/DISPATCH-TAG-OBJECT"',
            empty_check,
        )
        call = production.index(
            '"${production_argv[@]}" {recovery_lock_fd}>&- < "$dispatch_tag_snapshot"'
        )
        self.assertNotIn(
            'dispatch_tag_snapshot="$staging_production/',
            production,
        )
        self.assertLess(removal, empty_check)
        self.assertLess(empty_check, snapshot)
        self.assertLess(snapshot, call)

    def test_tmpfs_has_explicit_caps_derived_from_the_sealed_tree_limits(self) -> None:
        authority = json.loads(
            (
                REPO
                / "native/containment/native-shadow-mac3-successor-production-authority-arm64-v4.json"
            ).read_text(encoding="utf-8")
        )
        limits = authority["limits"]
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn(
            f"sealed_staging_max_total_bytes={limits['maxTotalBytes']}", source
        )
        self.assertIn(f"sealed_staging_max_entries={limits['maxEntries']}", source)
        self.assertIn(
            "staging_tmpfs_size_bytes=$((sealed_staging_max_total_bytes * 3))", source
        )
        self.assertIn(
            "staging_tmpfs_inodes=$((sealed_staging_max_entries * 3))", source
        )
        self.assertIn(
            'size=${staging_tmpfs_size_bytes},nr_inodes=${staging_tmpfs_inodes}', source
        )

    def test_tmpfs_identity_verifies_the_kernel_applied_caps(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        probe = source[
            source.index("read_tmpfs_mount_state()") :
            source.index("capture_tmpfs_mount_identity()")
        ]
        self.assertIn('"$staging_tmpfs_size_bytes"', probe)
        self.assertIn('"$staging_tmpfs_inodes"', probe)
        self.assertIn("expected_size_bytes = int(sys.argv[3])", probe)
        self.assertIn("expected_inodes = int(sys.argv[4])", probe)
        self.assertIn('option.startswith("size=")', probe)
        self.assertIn('option.startswith("nr_inodes=")', probe)
        self.assertIn("len(size_options) != 1", probe)
        self.assertIn("len(inode_options) != 1", probe)
        self.assertIn("observed_size_bytes != expected_size_bytes", probe)
        self.assertIn("observed_inodes != expected_inodes", probe)

    def test_production_requires_root_and_the_future_workflow_uses_one_sudo_edge(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        authority = source.index('"$PRODUCER" production-check')
        root_gate = source.index('[[ ${EUID} -eq 0 ]]', authority)
        scratch = source.index('scratch="$(mktemp -d', root_gate)
        mount = source.index('mount -t tmpfs', scratch)
        self.assertLess(authority, root_gate)
        self.assertLess(root_gate, scratch)
        self.assertLess(root_gate, mount)
        self.assertIn("production isolation must be installed as root", source)

        produce = workflow_job("produce")
        self.assertEqual(produce.count("sudo /usr/bin/env -i"), 1)
        self.assertNotIn(
            "sudo ./scripts/native-shadow-successor-produce-arm64-v5.sh", produce
        )
        self.assertIn('"$anchored_wrapper" --production', produce)
        rehearsal = workflow_job("free-rehearsal")
        self.assertNotIn("sudo ./scripts/native-shadow-successor-produce-arm64-v5.sh", rehearsal)

    def test_read_write_parent_is_canonical_private_empty_and_outside_sensitive_trees(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("require_dedicated_write_parent()", source)
        validator = source[
            source.index("require_dedicated_write_parent()") :
            source.index("isolation_prefix()")
        ]
        self.assertIn('[[ $candidate == /* ]]', validator)
        self.assertIn('[[ "/$candidate/" != *"/../"* ]]', validator)
        self.assertIn('canonical="$(readlink -f -- "$candidate")"', validator)
        self.assertIn('[[ $canonical == "$candidate" ]]', validator)
        for sensitive in ('"/"', '"/usr"', '"/etc"', '"/boot"', '"$ROOT"'):
            self.assertIn(sensitive, validator)
        self.assertIn('[[ $(stat -c %a -- "$candidate") == "700" ]]', validator)
        self.assertIn('[[ $(stat -c %u:%g -- "$candidate") == "0:0" ]]', validator)
        self.assertIn('find "$candidate" -mindepth 1 -maxdepth 1 -print -quit', validator)
        self.assertIn('[[ -z $first_member ]]', validator)

        workflow = workflow_job("produce")
        self.assertIn(
            '/usr/bin/install -d -o root -g root -m 0700 "$export_parent"',
            workflow,
        )
        self.assertIn('readonly export_parent="$export_tag/replica-$replica"', workflow)

        production = source.index('if [[ $mode == "produce" ]]; then')
        validation = source.index('require_dedicated_write_parent "$outputs_parent"', production)
        scratch = source.index('scratch="$(mktemp -d', validation)
        mount = source.index('mount -t tmpfs', scratch)
        self.assertLess(validation, scratch)
        self.assertLess(validation, mount)
        self.assertNotIn('--property=ReadWritePaths=/', source)

    def test_standalone_preflight_writes_only_to_its_validated_result_parent(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        host = source.index("require_host\n")
        validation = source.index(
            'require_dedicated_write_parent "$result_parent"', host
        )
        scratch = source.index('scratch="$(mktemp -d', validation)
        self.assertLess(validation, scratch)
        preflight_start = source.index('if [[ $mode == "preflight" ]]; then', scratch)
        preflight_end = source.index('if [[ $mode == "produce" ]]; then', preflight_start)
        preflight = source[preflight_start:preflight_end]
        self.assertIn('"--property=ReadWritePaths=$result_parent"', preflight)
        self.assertIn('--result "$result"', preflight)

    def test_qualification_runs_in_a_separate_minimal_loop_mount_unit(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("qualification_prefix()", source)
        prefix = source[
            source.index("qualification_prefix()") :
            source.index('if [[ $mode == "rehearsal" ]]')
        ]
        for property_value in (
            "PrivateNetwork=yes",
            "ProtectSystem=strict",
            "NoNewPrivileges=yes",
            "PrivateMounts=yes",
            "RestrictAddressFamilies=AF_UNIX",
            "PrivateDevices=no",
            "DevicePolicy=closed",
            "DeviceAllow=/dev/loop-control rw",
            "DeviceAllow=block-loop rw",
            "CapabilityBoundingSet=CAP_SYS_ADMIN",
            "AmbientCapabilities=",
        ):
            self.assertIn(f"--property={property_value}", prefix)
        for forbidden_capability in (
            "CAP_DAC_OVERRIDE",
            "CAP_MKNOD",
            "CAP_NET_ADMIN",
            "CAP_SYS_MODULE",
            "CAP_SYS_PTRACE",
        ):
            self.assertNotIn(forbidden_capability, prefix)

        production = source[source.index('if [[ $mode == "produce" ]]; then') :]
        self.assertIn("qualification_argv=()", production)
        self.assertIn('< <(qualification_prefix "$qualification_unit")', production)
        self.assertIn('"--property=ReadWritePaths=$outputs_parent"', production)
        isolated = production[
            production.index("qualification_argv=()") :
            production.index('"${qualification_argv[@]}"')
        ]
        self.assertIn('"$PRODUCER" qualify', isolated)
        self.assertEqual(production.count('"$PRODUCER" qualify'), 1)
        after_isolated_call = production[production.index('"${qualification_argv[@]}"') :]
        self.assertNotIn('python3 -I -S "$PRODUCER" qualify', after_isolated_call)

    def test_untrusted_units_have_no_capabilities_network_sockets_or_cross_process_control(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        isolation = source[
            source.index("isolation_prefix()") : source.index("qualification_prefix()")
        ]
        qualification = source[
            source.index("qualification_prefix()") : source.index(
                'if [[ $mode == "rehearsal" ]]'
            )
        ]
        self.assertIn("--property=CapabilityBoundingSet=", isolation)
        self.assertIn("--property=AmbientCapabilities=", isolation)
        self.assertIn("--property=RestrictAddressFamilies=AF_UNIX", isolation)
        self.assertNotIn("RestrictAddressFamilies=none", isolation)
        self.assertIn(
            "SystemCallFilter=~kill tkill tgkill pidfd_send_signal rt_sigqueueinfo "
            "rt_tgsigqueueinfo ptrace process_vm_readv process_vm_writev",
            isolation,
        )
        self.assertIn("--property=CapabilityBoundingSet=CAP_SYS_ADMIN", qualification)
        self.assertIn("--property=AmbientCapabilities=", qualification)
        self.assertIn("--property=RestrictAddressFamilies=AF_UNIX", qualification)
        self.assertNotIn("RestrictAddressFamilies=none", qualification)
        self.assertIn(
            "SystemCallFilter=~kill tkill tgkill pidfd_send_signal rt_sigqueueinfo "
            "rt_tgsigqueueinfo ptrace process_vm_readv process_vm_writev",
            qualification,
        )

    def test_consumed_success_or_diagnostic_is_collectable_without_copy_or_chown(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("publish_collectable_parent()", source)
        finalizer = source[
            source.index("publish_collectable_parent()") :
            source.index("require_recovery_output_parent()")
        ]
        self.assertIn('[[ $collectability_armed == "yes" ]] || return 0', finalizer)
        self.assertIn('marker="$outputs/ATTEMPT-CONSUMED.json"', finalizer)
        self.assertIn('[[ ! -e $marker && ! -L $marker ]] && return 0', finalizer)
        self.assertIn('[[ $(stat -c %d:%i -- "$outputs_parent") != "$outputs_parent_identity" ]]', finalizer)
        self.assertIn('"$PRODUCER" seal-replica-bundle', finalizer)
        self.assertIn('--repository-root "$ROOT"', finalizer)
        self.assertIn('--parent "$outputs_parent"', finalizer)
        self.assertIn('--parent-device "$parent_device"', finalizer)
        self.assertIn('--parent-inode "$parent_inode"', finalizer)
        self.assertIn('--successful no', finalizer)
        for forbidden in (
            "chown",
            "cp ",
            "rsync",
            "chmod -R",
            "chmod 0755",
            "find ",
            "chmod 0711",
        ):
            self.assertNotIn(forbidden, finalizer)

        validation = source.index('require_dedicated_write_parent "$outputs_parent"')
        identity = source.index('outputs_parent_identity="$(stat -c %d:%i', validation)
        armed = source.index('collectability_armed="yes"', identity)
        scratch = source.index('scratch="$(mktemp -d', armed)
        self.assertLess(validation, identity)
        self.assertLess(identity, armed)
        self.assertLess(armed, scratch)
        recovery_start = source.index("recover_production_state()")
        recovery = source[
            recovery_start : source.index("require_inputs()", recovery_start)
        ]
        unmount = recovery.index('umount "$staging"')
        publication = recovery.index("publish_collectable_parent", unmount)
        removal = recovery.index('"$PRODUCER" remove-verified-recovery', publication)
        self.assertLess(unmount, publication)
        self.assertLess(publication, removal)

        cleanup_start = source.rindex("cleanup()")
        cleanup = source[cleanup_start : source.index("trap cleanup EXIT", cleanup_start)]
        self.assertIn("recover_production_state", cleanup)

    def test_collectable_parent_contains_exactly_the_one_production_output(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        finalizer = source[
            source.index("publish_collectable_parent()") :
            source.index("require_recovery_output_parent()")
        ]
        self.assertIn('"$PRODUCER" seal-replica-bundle', finalizer)
        self.assertNotIn("find ", finalizer)
        producer = (
            REPO / "scripts/native_shadow_successor_produce_phase_arm64_v5.py"
        ).read_text(encoding="utf-8")
        self.assertIn(
            'expected_success_parent = frozenset(', producer
        )
        self.assertIn(
            'if parent_names != expected_success_parent:', producer
        )
        self.assertIn(
            'elif parent_names != frozenset(("outputs",)):', producer
        )
        self.assertNotIn("allowed_failure_parents", producer)

    def test_production_result_may_live_below_the_not_yet_created_output(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("require_inputs allow-missing-result-parent", source)
        self.assertIn("require_inputs require-result-parent", source)
        self.assertIn('[[ $result == "$outputs/PRODUCE-RESULT.json" ]]', source)
        self.assertEqual(
            source.count('[[ ${outputs##*/} == "outputs" ]]'),
            2,
        )
        self.assertIn('[[ ! -e $outputs && ! -L $outputs ]]', source)
        self.assertIn('[[ -d $outputs_parent && ! -L $outputs_parent ]]', source)

    def test_wrapper_cleanup_is_scoped_and_mount_failure_is_a_hard_stop(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        start = source.index("recover_production_state()")
        recovery = source[start : source.index("require_inputs()", start)]
        exact_scope = '[[ $scratch == "$production_scratch" ]]'
        self.assertIn(exact_scope, recovery)
        self.assertIn("read_tmpfs_mount_state", recovery)
        self.assertIn("require_absent_tmpfs_mount", recovery)
        self.assertNotIn("mountpoint -q", recovery)
        self.assertIn("umount", recovery)
        self.assertIn("production tmpfs cleanup failed", recovery)
        self.assertLess(
            recovery.index(exact_scope),
            recovery.index('"$PRODUCER" remove-verified-recovery'),
        )
        unmount = recovery.index('umount "$staging"')
        publication = recovery.index("publish_collectable_parent", unmount)
        self.assertLess(
            publication,
            recovery.index('"$PRODUCER" remove-verified-recovery', publication),
        )

    def test_production_transient_units_and_scratch_are_claim_scoped(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("initialise_production_recovery_identity()", source)
        start = source.index("initialise_production_recovery_identity()")
        identity = source[
            start : source.index("isolation_prefix()", start)
        ]
        self.assertIn('[[ $expected_tag_object_sha =~ ^[0-9a-f]{40}$ ]]', identity)
        self.assertIn('[[ $replica_ordinal =~ ^[12]$ ]]', identity)
        self.assertIn(
            'recovery_stem="boole-nsv5-${expected_tag_object_sha}-r${replica_ordinal}"',
            identity,
        )
        self.assertIn(
            'production_scratch="/run/boole/native-shadow-successor-v5/${recovery_stem}"',
            identity,
        )
        for variable, suffix in (
            ("preflight_unit", "preflight"),
            ("production_unit", "produce"),
            ("qualification_unit", "qualify"),
        ):
            self.assertIn(f'{variable}="${{recovery_stem}}-{suffix}.service"', identity)

        production = source[source.index('if [[ $mode == "produce" ]]; then') :]
        initialise = production.index("initialise_production_recovery_identity")
        scratch = production.index('scratch="$production_scratch"', initialise)
        refusal = production.index(
            '[[ ! -e $scratch && ! -L $scratch ]]', scratch
        )
        creation = production.index('mkdir -m 0700 -- "$scratch"', refusal)
        self.assertLess(initialise, scratch)
        self.assertLess(scratch, refusal)
        self.assertLess(refusal, creation)

    def test_production_recovery_root_has_a_real_root_owned_nonwritable_ancestry(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("require_root_recovery_parent()", source)
        validator = source[
            source.index("require_root_recovery_parent()") :
            source.index("require_dedicated_write_parent()")
        ]
        for path in (
            "/run",
            "/run/boole",
            "/run/boole/native-shadow-successor-v5",
        ):
            self.assertIn(path, validator)
        self.assertIn('[[ -d $candidate && ! -L $candidate ]]', validator)
        self.assertIn('[[ $(readlink -f -- "$candidate") == "$candidate" ]]', validator)
        self.assertIn('[[ $(stat -c %u:%g -- "$candidate") == "0:0" ]]', validator)
        self.assertIn('$((8#$permission_mode & 8#22))', validator)
        self.assertNotIn("mkdir -p", validator)

        production = source[source.index('if [[ $mode == "produce" ]]; then') :]
        root_gate = production.index("require_root_recovery_parent")
        scratch_refusal = production.index(
            '[[ ! -e $scratch && ! -L $scratch ]]', root_gate
        )
        scratch_creation = production.index(
            'mkdir -m 0700 -- "$scratch"', scratch_refusal
        )
        self.assertLess(root_gate, scratch_refusal)
        self.assertLess(scratch_refusal, scratch_creation)
        self.assertNotIn(
            "mkdir -p -m 0700 -- /run/boole/native-shadow-successor-v5",
            production,
        )

    def test_production_and_cleanup_hold_one_claim_replica_lock_before_any_scratch_unit_or_output_recovery(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        identity = source[
            source.index("initialise_production_recovery_identity()") :
            source.index("require_root_recovery_parent()")
        ]
        self.assertIn(
            'recovery_lock_path="/run/boole/native-shadow-successor-v5/${recovery_stem}.lock"',
            identity,
        )

        locker = source[
            source.index("acquire_production_recovery_lock()") :
            source.index("require_dedicated_write_parent()")
        ]
        for required in (
            "os.O_CREAT",
            "os.O_EXCL",
            "os.O_NOFOLLOW",
            "stat.S_ISREG",
            'metadata.st_uid != 0 or metadata.st_gid != 0',
            'stat.S_IMODE(metadata.st_mode) != 0o600',
            "metadata.st_nlink != 1",
            'exec {recovery_lock_fd}<>"$recovery_lock_path"',
            '/usr/bin/flock --exclusive --nonblock "$recovery_lock_fd"',
            '/proc/$$/fd/$recovery_lock_fd',
        ):
            self.assertIn(required, locker)
        self.assertNotIn('rm -f -- "$recovery_lock_path"', source)

        host = source[source.index("require_host()") : source.index("require_cleanup_host()")]
        cleanup_host = source[
            source.index("require_cleanup_host()") : source.index(
                "read_tmpfs_mount_state()"
            )
        ]
        self.assertIn("flock", host)
        self.assertIn("flock", cleanup_host)

        cleanup_start = source.index(
            'if [[ $mode == "cleanup-only" ]]', source.index("qualification_prefix()")
        )
        cleanup = source[
            cleanup_start :
            source.index('if [[ $mode == "rehearsal" ]]', cleanup_start)
        ]
        cleanup_identity = cleanup.index("initialise_production_recovery_identity")
        cleanup_parent = cleanup.index("require_root_recovery_parent", cleanup_identity)
        cleanup_lock = cleanup.index("acquire_production_recovery_lock", cleanup_parent)
        cleanup_output_recovery = cleanup.index(
            'require_recovery_output_parent "$outputs_parent"'
        )
        cleanup_scratch = cleanup.index('scratch="$production_scratch"', cleanup_lock)
        cleanup_recover = cleanup.index("recover_production_state", cleanup_scratch)
        self.assertLess(cleanup_identity, cleanup_parent)
        self.assertLess(cleanup_parent, cleanup_lock)
        self.assertLess(cleanup_lock, cleanup_output_recovery)
        self.assertLess(cleanup_output_recovery, cleanup_scratch)
        self.assertLess(cleanup_lock, cleanup_scratch)
        self.assertLess(cleanup_scratch, cleanup_recover)

        production = source[
            source.index('if [[ $mode == "produce" ]]; then', source.index("\nrequire_host\n")) :
        ]
        production_identity = production.index("initialise_production_recovery_identity")
        production_parent = production.index("require_root_recovery_parent", production_identity)
        production_lock = production.index("acquire_production_recovery_lock", production_parent)
        production_refusal = production.index(
            '[[ ! -e $scratch && ! -L $scratch ]]', production_lock
        )
        production_creation = production.index(
            'mkdir -m 0700 -- "$scratch"', production_refusal
        )
        self.assertLess(production_identity, production_parent)
        self.assertLess(production_parent, production_lock)
        self.assertLess(production_lock, production_refusal)
        self.assertLess(production_refusal, production_creation)

    def test_production_transient_children_never_inherit_the_recovery_lock_fd(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        production = source[source.index('if [[ $mode == "produce" ]]; then', 1450) :]
        lock = production.index("acquire_production_recovery_lock")
        locked = production[lock:]
        for invocation in (
            '"${preflight_argv[@]}" {recovery_lock_fd}>&-',
            '"${production_argv[@]}" {recovery_lock_fd}>&- < "$dispatch_tag_snapshot"',
            '"${qualification_argv[@]}" {recovery_lock_fd}>&-',
        ):
            self.assertIn(invocation, locked)

    def test_wrapper_supervisor_and_inner_claim_units_have_distinct_lifecycles(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        identity = source[
            source.index("initialise_production_recovery_identity()") :
            source.index("require_root_recovery_parent()")
        ]
        self.assertIn(
            'production_supervisor_unit="${recovery_stem}-supervisor.service"',
            identity,
        )
        self.assertIn(
            'cleanup_supervisor_unit="${recovery_stem}-cleanup.service"',
            identity,
        )
        membership = source[
            source.index("require_recovery_supervisor_membership()") :
            source.index("require_root_recovery_parent()")
        ]
        for required in (
            'expected_membership="/system.slice/$expected_unit"',
            'os.O_RDONLY | os.O_NOFOLLOW',
            'stat.S_ISREG',
            'len(raw) > 4096',
            'raw.decode("ascii")',
            "independent bounded cgroups",
        ):
            self.assertIn(required, membership)
        self.assertNotIn("every such child must share one systemd cgroup", membership)

        cleanup_start = source.index(
            'if [[ $mode == "cleanup-only" ]]', source.index("qualification_prefix()")
        )
        cleanup = source[
            cleanup_start : source.index('if [[ $mode == "rehearsal" ]]', cleanup_start)
        ]
        cleanup_identity = cleanup.index("initialise_production_recovery_identity")
        cleanup_membership = cleanup.index(
            'require_recovery_supervisor_membership "$cleanup_supervisor_unit"'
        )
        stop_producer = cleanup.index(
            'stop_and_verify_unit "$production_supervisor_unit"'
        )
        cleanup_lock = cleanup.index("acquire_production_recovery_lock")
        self.assertLess(cleanup_identity, cleanup_membership)
        self.assertLess(cleanup_membership, stop_producer)
        self.assertLess(stop_producer, cleanup_lock)

        production = source[
            source.index(
                'if [[ $mode == "produce" ]]; then', source.index("\nrequire_host\n")
            ) :
        ]
        production_identity = production.index("initialise_production_recovery_identity")
        production_membership = production.index(
            'require_recovery_supervisor_membership "$production_supervisor_unit"'
        )
        production_lock = production.index("acquire_production_recovery_lock")
        self.assertLess(production_identity, production_membership)
        self.assertLess(production_membership, production_lock)

        workflow = workflow_job("produce")
        self.assertNotIn("owns the\n      # entire process tree", workflow)
        self.assertIn("direct wrapper process tree", workflow)
        self.assertIn("claim-named transient services", workflow)
        for required in (
            'production_supervisor_unit="${recovery_stem}-supervisor.service"',
            'cleanup_supervisor_unit="${recovery_stem}-cleanup.service"',
            '--unit="$production_supervisor_unit"',
            '--unit="$cleanup_supervisor_unit"',
            '--property=KillMode=control-group',
            '"$anchored_wrapper" --production',
            '"$anchored_wrapper" --cleanup-only',
        ):
            self.assertIn(required, workflow)

        recovery_start = source.index("recover_production_state()")
        recovery = source[
            recovery_start : source.index("require_inputs()", recovery_start)
        ]
        for inner_unit in (
            "qualification_unit",
            "production_unit",
            "preflight_unit",
        ):
            self.assertIn(f'stop_and_verify_unit "${inner_unit}"', recovery)
        first_state_read = recovery.index("reconcile-output-state")
        self.assertLess(
            recovery.index('stop_and_verify_unit "$qualification_unit"'),
            first_state_read,
        )
        self.assertLess(
            recovery.index('stop_and_verify_unit "$production_unit"'),
            first_state_read,
        )
        self.assertLess(
            recovery.index('stop_and_verify_unit "$preflight_unit"'),
            first_state_read,
        )

    def test_linux_crashed_wrapper_cleanup_removes_all_claim_units(self) -> None:
        require_mode = os.environ.get("BOOLE_REQUIRE_V4_SYSTEMD_LIFECYCLE")
        self.assertIn(require_mode, (None, "1"))
        capability_failures = []
        if not sys.platform.startswith("linux"):
            capability_failures.append("host is not Linux")
        if getattr(os, "geteuid", lambda: -1)() != 0:
            capability_failures.append("test is not running as root")
        for command in ("systemd-run", "systemctl", "journalctl"):
            if shutil.which(command) is None:
                capability_failures.append(f"{command} is absent")
        if not capability_failures:
            systemd_probe = subprocess.run(
                ("systemctl", "show-environment"),
                check=False,
                text=True,
                capture_output=True,
            )
            if systemd_probe.returncode != 0:
                capability_failures.append("the systemd manager is unavailable")
        if capability_failures:
            detail = "; ".join(capability_failures)
            if require_mode == "1":
                self.fail(f"required v4 systemd lifecycle capability missing: {detail}")
            self.skipTest(detail)

        source = WRAPPER.read_text(encoding="utf-8")
        helper_start = source.index("bounded_systemd_control()")
        helper_end = source.index("initialise_production_recovery_identity()")
        helper_block = source[helper_start:helper_end]
        constant_names = {
            "transient_unit_stop_timeout_seconds",
            "transient_unit_gc_observations",
            "transient_unit_gc_interval_seconds",
            "transient_unit_gc_query_timeout_seconds",
            "transient_unit_gc_query_kill_after_seconds",
            "sealed_cleanup_deadline_seconds",
            "systemd_control_timeout_seconds",
            "systemd_control_kill_after_seconds",
        }
        constant_lines = [
            line
            for line in source.splitlines()
            if line.startswith("readonly ")
            and line.split("=", 1)[0].removeprefix("readonly ") in constant_names
        ]
        self.assertEqual(len(constant_lines), len(constant_names))

        unique = f"{os.getpid()}-{time.time_ns()}"
        stem = f"boole-nsv5-lifecycle-{unique}"
        outer = f"{stem}-supervisor.service"
        cleanup = f"{stem}-cleanup.service"
        preflight = f"{stem}-preflight.service"
        production = f"{stem}-produce.service"
        qualification = f"{stem}-qualify.service"
        all_units = (outer, cleanup, preflight, production, qualification)

        def run(*argv: str, timeout: int = 30) -> subprocess.CompletedProcess[str]:
            return subprocess.run(
                argv,
                check=False,
                text=True,
                capture_output=True,
                timeout=timeout,
            )

        def main_pid(unit: str) -> int:
            completed = run(
                "systemctl", "show", unit, "--property=MainPID", "--value"
            )
            if completed.returncode != 0 or not completed.stdout.strip():
                return 0
            return int(completed.stdout.strip(), 10)

        def wait_for_main_pid(unit: str) -> int:
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                observed = main_pid(unit)
                if observed > 0:
                    return observed
                time.sleep(0.1)
            details = run("journalctl", "-u", unit, "--no-pager", "-n", "80")
            self.fail(f"unit did not acquire a MainPID: {unit}\n{details.stdout}")

        def wait_for_absence(unit: str) -> None:
            deadline = time.monotonic() + 20
            cgroup = pathlib.Path("/sys/fs/cgroup/system.slice") / unit
            while time.monotonic() < deadline:
                state = run(
                    "systemctl", "show", unit, "--property=LoadState", "--value"
                )
                if state.stdout.strip() == "not-found" and not cgroup.exists():
                    self.assertEqual(main_pid(unit), 0)
                    return
                time.sleep(0.1)
            self.fail(f"claim unit remained after cleanup: {unit}")

        with tempfile.TemporaryDirectory(prefix="boole-nsv5-lifecycle-") as directory:
            root = pathlib.Path(directory)
            ready = root / "READY"
            fixture = root / "outer-fixture.sh"
            fixture.write_text(
                "#!/usr/bin/env bash\n"
                "set -euo pipefail\n"
                "ready=$1\n"
                "shift\n"
                "for unit in \"$@\"; do\n"
                "  /usr/bin/systemd-run --unit=\"$unit\" --no-block --collect "
                "--service-type=exec --property=KillMode=control-group "
                "--property=TimeoutStopSec=5s --property=SendSIGKILL=yes "
                "--property=Restart=no --property=RuntimeMaxSec=120s -- "
                "/usr/bin/sleep 120\n"
                "done\n"
                "/usr/bin/touch \"$ready\"\n"
                "/usr/bin/sleep 120\n",
                encoding="utf-8",
            )
            fixture.chmod(0o700)
            cleanup_fixture = root / "cleanup-fixture.sh"
            cleanup_fixture.write_text(
                "#!/usr/bin/env bash\n"
                "set -euo pipefail\n"
                "readonly PATH=/usr/sbin:/usr/bin:/sbin:/bin\n"
                "export PATH\n"
                + "\n".join(constant_lines)
                + "\n"
                + helper_block
                + "\nfor unit in \"$@\"; do\n"
                "  stop_and_verify_unit \"$unit\"\n"
                "done\n",
                encoding="utf-8",
            )
            cleanup_fixture.chmod(0o700)

            try:
                started = run(
                    "systemd-run",
                    f"--unit={outer}",
                    "--no-block",
                    "--collect",
                    "--service-type=exec",
                    "--property=KillMode=control-group",
                    "--property=TimeoutStopSec=5s",
                    "--property=SendSIGKILL=yes",
                    "--property=Restart=no",
                    "--property=RuntimeMaxSec=120s",
                    "--",
                    str(fixture),
                    str(ready),
                    preflight,
                    production,
                    qualification,
                )
                self.assertEqual(started.returncode, 0, started.stderr)
                outer_pid = wait_for_main_pid(outer)
                deadline = time.monotonic() + 15
                while not ready.exists() and time.monotonic() < deadline:
                    time.sleep(0.1)
                self.assertTrue(ready.is_file(), "the outer fixture never became ready")

                self.assertEqual(
                    pathlib.Path(f"/proc/{outer_pid}/cgroup").read_text(
                        encoding="ascii"
                    ),
                    f"0::/system.slice/{outer}\n",
                )
                for inner in (preflight, production, qualification):
                    pid = wait_for_main_pid(inner)
                    self.assertEqual(
                        pathlib.Path(f"/proc/{pid}/cgroup").read_text(
                            encoding="ascii"
                        ),
                        f"0::/system.slice/{inner}\n",
                    )

                crashed = run(
                    "systemctl",
                    "kill",
                    "--kill-whom=all",
                    "--signal=SIGKILL",
                    outer,
                )
                self.assertEqual(crashed.returncode, 0, crashed.stderr)
                deadline = time.monotonic() + 15
                while main_pid(outer) != 0 and time.monotonic() < deadline:
                    time.sleep(0.1)
                self.assertEqual(main_pid(outer), 0)

                cleaned = run(
                    "systemd-run",
                    f"--unit={cleanup}",
                    "--pipe",
                    "--wait",
                    "--collect",
                    "--service-type=exec",
                    "--property=KillMode=control-group",
                    "--property=RuntimeMaxSec=90s",
                    "--",
                    str(cleanup_fixture),
                    outer,
                    qualification,
                    production,
                    preflight,
                    timeout=120,
                )
                self.assertEqual(
                    cleaned.returncode,
                    0,
                    f"{cleaned.stdout}\n{cleaned.stderr}",
                )
                for unit in all_units:
                    wait_for_absence(unit)
            finally:
                for unit in reversed(all_units):
                    run("systemctl", "stop", unit)
                    run("systemctl", "reset-failed", unit)

    def test_linux_exact_rehearsal_systemd_property_envelope_is_accepted(self) -> None:
        """Name the first property that a real systemd manager rejects.

        The free R2 rehearsal installs this envelope before it can assemble a
        staging tree.  Static source tests cannot prove that the target
        manager accepts every property, so the required Linux gate applies the
        exact ordered prefix one property at a time without creating images or
        attempt markers.
        """

        require_mode = os.environ.get("BOOLE_REQUIRE_V4_SYSTEMD_ENVELOPE")
        self.assertIn(require_mode, (None, "1"))
        capability_failures = []
        if not sys.platform.startswith("linux"):
            capability_failures.append("host is not Linux")
        if getattr(os, "geteuid", lambda: -1)() != 0:
            capability_failures.append("test is not running as root")
        for command in ("systemd-run", "systemctl"):
            if shutil.which(command) is None:
                capability_failures.append(f"{command} is absent")
        if not capability_failures:
            systemd_probe = subprocess.run(
                ("systemctl", "show-environment"),
                check=False,
                text=True,
                capture_output=True,
            )
            if systemd_probe.returncode != 0:
                capability_failures.append("the systemd manager is unavailable")
        properties = (
            "PrivateNetwork=yes",
            "ProtectSystem=strict",
            "NoNewPrivileges=yes",
            "KillMode=control-group",
            "TimeoutStopSec=20s",
            "SendSIGKILL=yes",
            "Restart=no",
            "MemoryAccounting=yes",
            "MemoryMax=8589934592",
            "MemorySwapMax=0",
            "TasksAccounting=yes",
            "TasksMax=128",
            "CPUAccounting=yes",
            "RuntimeMaxSec=1200s",
            "OOMPolicy=kill",
            "PrivateDevices=yes",
            "PrivateMounts=yes",
            "RestrictAddressFamilies=AF_UNIX",
            "CapabilityBoundingSet=",
            "AmbientCapabilities=",
            "SystemCallFilter=~kill tkill tgkill pidfd_send_signal "
            "rt_sigqueueinfo rt_tgsigqueueinfo ptrace process_vm_readv "
            "process_vm_writev",
        )
        source_properties = (
            "PrivateNetwork=yes",
            "ProtectSystem=strict",
            "NoNewPrivileges=yes",
            "KillMode=control-group",
            "TimeoutStopSec=${transient_unit_stop_timeout_seconds}s",
            "SendSIGKILL=yes",
            "Restart=no",
            "MemoryAccounting=yes",
            "MemoryMax=${staging_unit_memory_max_bytes}",
            "MemorySwapMax=0",
            "TasksAccounting=yes",
            "TasksMax=${staging_unit_tasks_max}",
            "CPUAccounting=yes",
            "RuntimeMaxSec=${staging_unit_runtime_max_seconds}s",
            "OOMPolicy=kill",
            "PrivateDevices=yes",
            "PrivateMounts=yes",
            "RestrictAddressFamilies=AF_UNIX",
            "CapabilityBoundingSet=",
            "AmbientCapabilities=",
            "SystemCallFilter=~kill tkill tgkill pidfd_send_signal "
            "rt_sigqueueinfo rt_tgsigqueueinfo ptrace process_vm_readv "
            "process_vm_writev",
        )
        self.assertEqual(len(source_properties), len(properties))
        source = WRAPPER.read_text(encoding="utf-8")
        isolation = source[
            source.index("isolation_prefix()") : source.index("qualification_prefix()")
        ]
        cursor = -1
        for property_value in source_properties:
            needle = property_value
            observed = isolation.index(needle)
            self.assertGreater(observed, cursor, f"property is out of order: {needle}")
            cursor = observed

        if capability_failures:
            detail = "; ".join(capability_failures)
            if require_mode == "1":
                self.fail(f"required v4 systemd envelope capability missing: {detail}")
            self.skipTest(detail)

        unique = f"{os.getpid()}-{time.time_ns()}"
        with tempfile.TemporaryDirectory(prefix="boole-nsv5-envelope-") as directory:
            writable_one = pathlib.Path(directory) / "scratch"
            writable_two = pathlib.Path(directory) / "result"
            writable_one.mkdir()
            writable_two.mkdir()
            exact_properties = properties + (
                f"ReadWritePaths={writable_one}",
                f"ReadWritePaths={writable_two}",
            )
            for property_count in range(1, len(exact_properties) + 1):
                property_value = exact_properties[property_count - 1]
                unit = f"boole-nsv5-envelope-{unique}-{property_count}.service"
                argv = [
                    "systemd-run",
                    f"--unit={unit}",
                    "--pipe",
                    "--wait",
                    "--collect",
                    "--service-type=exec",
                ]
                argv.extend(
                    f"--property={value}"
                    for value in exact_properties[:property_count]
                )
                argv.extend(("--", "/usr/bin/true"))
                try:
                    completed = subprocess.run(
                        argv,
                        check=False,
                        text=True,
                        capture_output=True,
                        timeout=30,
                    )
                    self.assertEqual(
                        completed.returncode,
                        0,
                        "v4 rehearsal systemd envelope first failed after "
                        f"property {property_count}/{len(exact_properties)} "
                        f"({property_value!r})\nstdout:\n{completed.stdout}\n"
                        f"stderr:\n{completed.stderr}",
                    )
                finally:
                    subprocess.run(
                        ("systemctl", "stop", unit),
                        check=False,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                    )
                    subprocess.run(
                        ("systemctl", "reset-failed", unit),
                        check=False,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                    )

            network_unit = f"boole-nsv5-envelope-{unique}-network.service"
            network_probe = (
                "import socket\n"
                "local = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)\n"
                "local.close()\n"
                "for family in (socket.AF_INET, socket.AF_INET6):\n"
                "    try:\n"
                "        candidate = socket.socket(family, socket.SOCK_STREAM)\n"
                "    except OSError:\n"
                "        continue\n"
                "    candidate.close()\n"
                "    raise SystemExit(f'network socket family unexpectedly opened: {family}')\n"
            )
            network_argv = [
                "systemd-run",
                f"--unit={network_unit}",
                "--pipe",
                "--wait",
                "--collect",
                "--service-type=exec",
            ]
            network_argv.extend(
                f"--property={value}" for value in exact_properties
            )
            network_argv.extend(("--", "/usr/bin/python3", "-I", "-S", "-c", network_probe))
            try:
                completed = subprocess.run(
                    network_argv,
                    check=False,
                    text=True,
                    capture_output=True,
                    timeout=30,
                )
                self.assertEqual(
                    completed.returncode,
                    0,
                    "the accepted v4 envelope did not preserve AF_UNIX while "
                    "blocking AF_INET/AF_INET6\n"
                    f"stdout:\n{completed.stdout}\nstderr:\n{completed.stderr}",
                )
            finally:
                subprocess.run(
                    ("systemctl", "stop", network_unit),
                    check=False,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                subprocess.run(
                    ("systemctl", "reset-failed", network_unit),
                    check=False,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )

    @unittest.skipUnless(
        sys.platform.startswith("linux")
        and shutil.which("flock") is not None
        and shutil.which("bash") is not None,
        "Linux flock and modern Bash are required",
    )
    def test_closed_child_fd_releases_flock_when_the_parent_dies(self) -> None:
        bash = pathlib.Path(shutil.which("bash") or "")
        version = subprocess.run(
            [str(bash), "-c", "printf '%s' \"${BASH_VERSINFO[0]}\""],
            check=True,
            stdout=subprocess.PIPE,
            text=True,
        ).stdout
        if int(version) < 4:
            self.skipTest("dynamic Bash descriptors require Bash 4+")
        with tempfile.TemporaryDirectory() as temporary:
            lock = pathlib.Path(temporary) / "claim.lock"
            script = "\n".join(
                (
                    "set -euo pipefail",
                    'exec {lock_fd}<>"$1"',
                    '/usr/bin/flock --exclusive --nonblock "$lock_fd"',
                    "/usr/bin/python3 -c 'import signal,time; "
                    "signal.signal(signal.SIGHUP, signal.SIG_IGN); time.sleep(30)' "
                    "{lock_fd}>&- &",
                    'printf \'%s\\n\' "$!"',
                    "wait",
                )
            )
            parent = subprocess.Popen(
                [str(bash), "-c", script, "bash", str(lock)],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
            )
            assert parent.stdout is not None
            child_pid = int(parent.stdout.readline().strip())
            os.kill(child_pid, 0)
            parent.kill()
            parent.wait(timeout=5)
            try:
                claimant = subprocess.run(
                    [
                        "/usr/bin/flock",
                        "--exclusive",
                        "--nonblock",
                        str(lock),
                        "/usr/bin/true",
                    ],
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=5,
                )
                self.assertEqual(claimant.returncode, 0, claimant.stderr.decode())
                os.kill(child_pid, 0)
            finally:
                try:
                    os.kill(child_pid, 9)
                except ProcessLookupError:
                    pass

    def test_transient_units_have_names_and_bounded_cgroup_stop_contract(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        for function in ("isolation_prefix()", "qualification_prefix()"):
            start = source.index(function)
            following = source.index("()", start + len(function))
            block = source[start:following]
            self.assertIn("local unit_name=$1", block)
            self.assertIn('--unit="$unit_name"', block)
            for property_value in (
                "KillMode=control-group",
                "SendSIGKILL=yes",
                "Restart=no",
            ):
                self.assertIn(f"--property={property_value}", block)
            self.assertIn(
                '--property="TimeoutStopSec=${transient_unit_stop_timeout_seconds}s"',
                block,
            )

        production = source[source.index('if [[ $mode == "produce" ]]; then') :]
        self.assertIn('< <(isolation_prefix "$preflight_unit")', production)
        self.assertIn('< <(isolation_prefix "$production_unit")', production)
        self.assertIn('< <(qualification_prefix "$qualification_unit")', production)

    def test_each_successful_transient_unit_is_absent_before_next_phase(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")

        rehearsal = source[
            source.index('if [[ $mode == "rehearsal" ]]') :
            source.index("# Both production preflight and production require")
        ]
        rehearsal_call = rehearsal.index('"${rehearsal_argv[@]}"')
        rehearsal_stop = rehearsal.index(
            'stop_and_verify_unit "$rehearsal_unit"', rehearsal_call
        )
        rehearsal_publish = rehearsal.index(
            '[[ -f $result && ! -L $result ]]', rehearsal_stop
        )
        self.assertLess(rehearsal_call, rehearsal_stop)
        self.assertLess(rehearsal_stop, rehearsal_publish)

        standalone_start = source.rindex('if [[ $mode == "preflight" ]]; then')
        standalone = source[
            standalone_start :
            source.index('if [[ $mode == "produce" ]]; then', standalone_start)
        ]
        standalone_call = standalone.index('"${preflight_argv[@]}"')
        standalone_stop = standalone.index(
            'stop_and_verify_unit "$preflight_unit"', standalone_call
        )
        standalone_exit = standalone.index("exit 0", standalone_stop)
        self.assertLess(standalone_call, standalone_stop)
        self.assertLess(standalone_stop, standalone_exit)

        production = source[source.rindex('if [[ $mode == "produce" ]]; then') :]
        preflight_call = production.index('"${preflight_argv[@]}"')
        preflight_stop = production.index(
            'stop_and_verify_unit "$preflight_unit"', preflight_call
        )
        preflight_removal = production.index(
            'rm -rf -- "$staging_preflight"', preflight_stop
        )
        production_call = production.index(
            '"${production_argv[@]}" {recovery_lock_fd}>&- < "$dispatch_tag_snapshot"',
            preflight_removal,
        )
        production_stop = production.index(
            'stop_and_verify_unit "$production_unit"', production_call
        )
        qualification_call = production.index(
            '"${qualification_argv[@]}"', production_stop
        )
        qualification_stop = production.index(
            'stop_and_verify_unit "$qualification_unit"', qualification_call
        )
        provenance = production.index("recheck_dispatch_claim_ref", qualification_stop)
        self.assertLess(preflight_call, preflight_stop)
        self.assertLess(preflight_stop, preflight_removal)
        self.assertLess(preflight_removal, production_call)
        self.assertLess(production_call, production_stop)
        self.assertLess(production_stop, qualification_call)
        self.assertLess(qualification_call, qualification_stop)
        self.assertLess(qualification_stop, provenance)

    def test_transient_units_have_finite_resource_contracts(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        workflow = WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(
            "staging_unit_memory_max_bytes=$((staging_tmpfs_size_bytes + sealed_staging_max_total_bytes))",
            source,
        )
        self.assertIn("readonly staging_unit_tasks_max=128", source)
        self.assertIn("readonly staging_unit_runtime_max_seconds=1200", source)
        self.assertIn("outer supervisor caps their aggregate at fifty minutes", source)
        self.assertIn(
            "readonly production_supervisor_runtime_seconds=3000", workflow
        )
        self.assertIn("readonly transient_unit_stop_timeout_seconds=20", source)
        self.assertIn("readonly sealed_cleanup_deadline_seconds=10", source)
        self.assertIn(
            "systemd_run_client_timeout_seconds=$((staging_unit_runtime_max_seconds + transient_unit_stop_timeout_seconds + sealed_cleanup_deadline_seconds))",
            source,
        )

        for function in ("isolation_prefix()", "qualification_prefix()"):
            start = source.index(function)
            following = source.index("()", start + len(function))
            block = source[start:following]
            for property_value in (
                "MemoryAccounting=yes",
                "MemorySwapMax=0",
                "TasksAccounting=yes",
                "CPUAccounting=yes",
                "OOMPolicy=kill",
            ):
                self.assertIn(f"--property={property_value}", block)
            self.assertIn(
                '--property="MemoryMax=${staging_unit_memory_max_bytes}"', block
            )
            self.assertIn('--property="TasksMax=${staging_unit_tasks_max}"', block)
            self.assertIn(
                '--property="RuntimeMaxSec=${staging_unit_runtime_max_seconds}s"',
                block,
            )
            for forbidden in (
                "MemoryMax=infinity",
                "MemorySwapMax=infinity",
                "TasksMax=infinity",
                "RuntimeMaxSec=infinity",
            ):
                self.assertNotIn(forbidden, block)

    def test_rehearsal_binds_r2_to_its_exact_unit_and_honest_cap_evidence(
        self,
    ) -> None:
        wrapper = WRAPPER.read_text(encoding="utf-8")
        rehearsal = wrapper[
            wrapper.index('if [[ $mode == "rehearsal" ]]') :
            wrapper.index("# Both production preflight and production require")
        ]
        self.assertIn('--expected-systemd-unit "$rehearsal_unit"', rehearsal)

        core = (
            REPO / "scripts/native_shadow_successor_produce_phase_arm64_v5.py"
        ).read_text(encoding="utf-8")
        for contract in (
            "R3_MEMORY_MAX_BYTES = 8 * 1024 * 1024 * 1024",
            "R3_MEMORY_SWAP_MAX_BYTES = 0",
            "R3_PIDS_MAX = 128",
            "R3_RUNTIME_MAX_SECONDS = 1200",
            '"equalAtBeforeAndAfterObservations": True',
            '"leafControlsKernelObserved": True',
            '"limitEventsKernelObserved": True',
            '"requestedUnitMembershipMatched": True',
            '"memoryHighEvents": 0',
            '"memoryMaxEvents": 0',
            '"memoryOomEvents": 0',
            '"memoryOomKillEvents": 0',
            '"pidsMaxEvents": 0',
            '"execReachedRequestedUnit": True',
            '"kernelObserved": False',
            '"managerValueQueried": False',
            '"sourcePinnedRequestPresent": True',
            '"memory.events.local"',
            '"pids.events"',
        ):
            self.assertIn(contract, core)
        self.assertGreaterEqual(
            core.count("_read_cgroup_execution_observation("), 3
        )
        self.assertNotIn('"cgroupPath"', core)
        self.assertNotIn('"unitName"', core)

    def test_systemd_control_clients_have_a_finite_outer_timeout(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn(
            "systemd_control_timeout_seconds=$((transient_unit_stop_timeout_seconds + sealed_cleanup_deadline_seconds))",
            source,
        )
        self.assertIn(
            "readonly systemd_control_kill_after_seconds=$sealed_cleanup_deadline_seconds",
            source,
        )

        host = source[source.index("require_host()") : source.index("require_cleanup_host()")]
        cleanup_host = source[
            source.index("require_cleanup_host()") : source.index(
                "read_tmpfs_mount_state()"
            )
        ]
        self.assertIn("timeout", host)
        self.assertIn("timeout", cleanup_host)

        helper = source[
            source.index("bounded_systemd_control()") : source.index(
                "stop_and_verify_unit()"
            )
        ]
        self.assertIn("/usr/bin/timeout", helper)
        self.assertIn("--foreground", helper)
        self.assertIn('--signal=TERM', helper)
        self.assertIn(
            '--kill-after="${systemd_control_kill_after_seconds}s"', helper
        )
        self.assertIn('"${systemd_control_timeout_seconds}s"', helper)
        self.assertIn(
            '--kill-after="${transient_unit_gc_query_kill_after_seconds}s"', helper
        )
        self.assertIn('"${transient_unit_gc_query_timeout_seconds}s"', helper)
        self.assertIn('"$@"', helper)

        stopper = source[
            source.index("stop_and_verify_unit()") :
            source.index("initialise_production_recovery_identity()")
        ]
        for command in (
            'bounded_systemd_gc_control systemctl show "$unit_name" --property=LoadState --value',
            'bounded_systemd_control systemctl stop "$unit_name"',
            "bounded_systemd_control journalctl --sync",
            'bounded_systemd_gc_control systemctl show "$unit_name" --property=MainPID --value',
            'bounded_systemd_gc_control systemctl reset-failed "$unit_name"',
        ):
            self.assertIn(command, stopper)
        self.assertNotIn('load_state="$(systemctl ', stopper)
        self.assertNotIn('main_pid="$(systemctl ', stopper)
        self.assertNotIn("\n  systemctl ", stopper)
        self.assertNotIn("\n  journalctl ", stopper)

    def test_transient_unit_gc_is_polled_with_one_small_fixed_bound(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("readonly transient_unit_gc_observations=11", source)
        self.assertIn("readonly transient_unit_gc_interval_seconds=1", source)
        self.assertIn("readonly transient_unit_gc_query_timeout_seconds=2", source)
        self.assertIn("readonly transient_unit_gc_query_kill_after_seconds=2", source)
        self.assertIn("bounded_systemd_gc_control()", source)
        helper = source[
            source.index("wait_for_unit_absence()") :
            source.index("stop_and_verify_unit()")
        ]
        self.assertIn("attempt < transient_unit_gc_observations", helper)
        self.assertIn(
            '[[ $load_state == "not-found" && ! -e "/sys/fs/cgroup/system.slice/$unit_name" ]]',
            helper,
        )
        self.assertIn(
            '/usr/bin/sleep "$transient_unit_gc_interval_seconds"', helper
        )
        self.assertNotIn("while true", helper)

    def test_transient_unit_gc_races_converge_only_after_absence_is_proved(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        functions = source[
            source.index("wait_for_unit_absence()") :
            source.index("initialise_production_recovery_identity()")
        ]
        harness = (
            "set -euo pipefail\n"
            "readonly transient_unit_gc_observations=2\n"
            "readonly transient_unit_gc_interval_seconds=0\n"
            "STOP_SEEN=no\n"
            "bounded_systemd_control() {\n"
            "  if [[ $1 == journalctl ]]; then return 0; fi\n"
            "  if [[ $1 != systemctl ]]; then return 91; fi\n"
            "  shift\n"
            "  case ${CASE:?} in\n"
            "    stop-gc)\n"
            "      if [[ $1 == stop ]]; then STOP_SEEN=yes; return 1; fi\n"
            "      if [[ $* == *MainPID* ]]; then printf '0\\n'; return 0; fi\n"
            "      if [[ $STOP_SEEN == yes ]]; then printf 'not-found\\n'; else printf 'loaded\\n'; fi\n"
            "      ;;\n"
            "    mainpid-gc)\n"
            "      if [[ $1 == stop ]]; then STOP_SEEN=yes; return 0; fi\n"
            "      if [[ $* == *MainPID* ]]; then return 1; fi\n"
            "      if [[ $STOP_SEEN == yes ]]; then printf 'not-found\\n'; else printf 'loaded\\n'; fi\n"
            "      ;;\n"
            "    still-present)\n"
            "      if [[ $1 == stop ]]; then return 1; fi\n"
            "      printf 'loaded\\n'\n"
            "      ;;\n"
            "    *) return 92;;\n"
            "  esac\n"
            "}\n"
            "bounded_systemd_gc_control() { bounded_systemd_control \"$@\"; }\n"
            f"{functions}\n"
            "stop_and_verify_unit boole-nsv5-test-race.service\n"
        )
        for case, expected in (
            ("stop-gc", 0),
            ("mainpid-gc", 0),
            ("still-present", 1),
        ):
            with self.subTest(case=case):
                completed = subprocess.run(
                    ["bash", "-c", harness],
                    cwd=REPO,
                    env={**os.environ, "CASE": case},
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    check=False,
                )
                self.assertEqual(completed.returncode, expected, completed.stderr)

    def test_systemd_run_wait_client_ends_before_the_workflow_job(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        for function in ("isolation_prefix()", "qualification_prefix()"):
            start = source.index(function)
            following = source.index("()", start + len(function))
            block = source[start:following]
            self.assertIn("/usr/bin/timeout", block)
            self.assertIn("--foreground", block)
            self.assertIn("--signal=TERM", block)
            self.assertIn(
                '--kill-after="${sealed_cleanup_deadline_seconds}s"', block
            )
            self.assertIn(
                '"${systemd_run_client_timeout_seconds}s"', block
            )
            self.assertLess(block.index("/usr/bin/timeout"), block.index("systemd-run"))

        produce_job = workflow_job("produce")
        timeout_match = re.search(
            r"^    timeout-minutes: ([0-9]+)$", produce_job, re.MULTILINE
        )
        self.assertIsNotNone(timeout_match)
        job_minutes = int(timeout_match.group(1))
        self.assertEqual(job_minutes, 90)
        systemd_run_client_max_seconds = 1200 + 20 + 10 + 10
        normal_control_max_seconds = 30 + 10
        gc_control_max_seconds = 2 + 2
        one_gc_poll_max_seconds = 11 * gc_control_max_seconds + 10 * 1
        one_stop_verification_max_seconds = (
            3 * gc_control_max_seconds
            + 2 * normal_control_max_seconds
            + one_gc_poll_max_seconds
        )
        three_unit_seconds = 3 * (
            systemd_run_client_max_seconds + one_stop_verification_max_seconds
        )
        outer_supervisor_seconds = 3000
        self.assertLess(
            three_unit_seconds,
            70 * 60,
            "the individual unit ceilings must remain finite",
        )
        self.assertLess(
            outer_supervisor_seconds,
            three_unit_seconds,
            "the outer supervisor must be the tighter aggregate ceiling",
        )
        self.assertGreaterEqual(
            job_minutes * 60 - outer_supervisor_seconds,
            40 * 60,
            "the aggregate supervisor must leave forty minutes to the job",
        )

    def test_signal_cleanup_stops_units_before_unmount_and_collection(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("stop_and_verify_unit()", source)
        absence_waiter = source[
            source.index("wait_for_unit_absence()") :
            source.index("stop_and_verify_unit()")
        ]
        stopper = source[
            source.index("stop_and_verify_unit()") :
            source.index("initialise_production_recovery_identity()")
        ]
        self.assertIn('systemctl stop "$unit_name"', stopper)
        self.assertIn("--property=MainPID --value", stopper)
        self.assertIn('if [[ $main_pid != "0" ]]; then', stopper)
        self.assertIn('/sys/fs/cgroup/system.slice/$unit_name', absence_waiter)
        self.assertIn('systemctl reset-failed "$unit_name"', stopper)
        self.assertIn("LoadState --value", absence_waiter)
        self.assertIn('[[ $load_state == "not-found"', absence_waiter)
        self.assertIn("journalctl --sync", stopper)
        self.assertIn(
            'if [[ $load_state == "not-found" ]]; then\n'
            '    wait_for_unit_absence "$unit_name"\n'
            '    return $?\n'
            "  fi",
            stopper,
        )
        self.assertGreaterEqual(stopper.count('wait_for_unit_absence "$unit_name"'), 3)

        recovery_start = source.index("recover_production_state()")
        recovery = source[recovery_start : source.index("require_inputs()", recovery_start)]
        qualification = recovery.index('stop_and_verify_unit "$qualification_unit"')
        production = recovery.index('stop_and_verify_unit "$production_unit"')
        preflight = recovery.index('stop_and_verify_unit "$preflight_unit"')
        unmount = recovery.index('umount "$staging"')
        publication = recovery.index("publish_collectable_parent", unmount)
        self.assertLess(qualification, production)
        self.assertLess(production, preflight)
        self.assertLess(preflight, unmount)
        self.assertLess(unmount, publication)
        cleanup_start = source.rindex("cleanup()")
        cleanup = source[cleanup_start : source.index("trap cleanup EXIT", cleanup_start)]
        self.assertIn("recover_production_state", cleanup)
        self.assertIn("trap - EXIT TERM INT HUP", cleanup)
        self.assertIn("trap 'exit 143' TERM", source)
        self.assertIn("trap 'exit 130' INT", source)
        self.assertIn("trap 'exit 129' HUP", source)

    def test_tmpfs_cleanup_rechecks_the_exact_mount_identity(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("capture_tmpfs_mount_identity()", source)
        capture = source[
            source.index("read_tmpfs_mount_state()") :
            source.index("stop_and_verify_unit()")
        ]
        self.assertIn("local expected_source=$2", capture)
        self.assertIn("/proc/self/mountinfo", capture)
        self.assertIn('record["fileSystemType"] != "tmpfs"', capture)
        self.assertIn('record["source"] != expected_source', capture)
        self.assertIn('required_options = {"rw", "nodev", "nosuid"}', capture)

        production_mount = source.rindex("mount -t tmpfs")
        production_mount_block = source[production_mount : production_mount + 300]
        self.assertIn('"$expected_mount_source" "$staging"', production_mount_block)
        identity = source.index(
            'staging_mount_identity="$(capture_tmpfs_mount_identity "$staging" "$expected_mount_source")"',
            production_mount,
        )
        children = source.index(
            'mkdir -m 0700 "$staging_preflight" "$staging_production"',
            identity,
        )
        self.assertLess(production_mount, identity)
        self.assertLess(identity, children)

        cleanup_start = source.rindex("cleanup()")
        cleanup = source[cleanup_start : source.index("trap cleanup EXIT", cleanup_start)]
        current = cleanup.index(
            'current_mount_identity="$(capture_tmpfs_mount_identity "$staging" "$expected_mount_source")"'
        )
        comparison = cleanup.index(
            'elif [[ $current_mount_identity != "$staging_mount_identity" ]]', current
        )
        unmount = cleanup.index('umount "$staging"', comparison)
        self.assertLess(current, comparison)
        self.assertLess(comparison, unmount)

    def test_post_unmount_is_proved_by_a_three_state_mountinfo_probe(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("read_tmpfs_mount_state()", source)
        probe = source[
            source.index("read_tmpfs_mount_state()") :
            source.index("capture_tmpfs_mount_identity()")
        ]
        self.assertIn('/proc/self/mountinfo', probe)
        self.assertIn('sys.stdout.write("absent")', probe)
        self.assertIn('if len(matches) == 0:', probe)
        self.assertIn('if len(matches) != 1:', probe)

        recovery = source[
            source.index("recover_production_state()") :
            source.index("require_inputs()")
        ]
        self.assertNotIn("mountpoint -q", recovery)
        self.assertIn(
            'mount_state="$(read_tmpfs_mount_state "$staging" "$expected_mount_source")"',
            recovery,
        )
        self.assertGreaterEqual(
            recovery.count(
                'require_absent_tmpfs_mount "$staging" "$expected_mount_source"'
            ),
            3,
        )
        post_unmount = recovery.index('"$PRODUCER" verify-recovery-after-unmount')
        before_publication = recovery.index(
            'require_absent_tmpfs_mount "$staging" "$expected_mount_source"',
            post_unmount,
        )
        publication = min(
            recovery.index("publish_collectable_parent", before_publication),
            recovery.index("resume_successful_parent", before_publication),
        )
        self.assertLess(post_unmount, before_publication)
        self.assertLess(before_publication, publication)

    def test_production_recovery_record_precedes_children_and_gates_unmount(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        production_mount = source.rindex("mount -t tmpfs")
        capture = source.index(
            'staging_mount_identity="$(capture_tmpfs_mount_identity',
            production_mount,
        )
        publish = source.index('"$PRODUCER" publish-recovery-record', capture)
        children = source.index(
            'mkdir -m 0700 "$staging_preflight" "$staging_production"',
            publish,
        )
        first_unit = source.index('"${preflight_argv[@]}"', children)
        self.assertLess(production_mount, capture)
        self.assertLess(capture, publish)
        self.assertLess(publish, children)
        self.assertLess(children, first_unit)

        recovery_start = source.index("recover_production_state()")
        recovery = source[recovery_start : source.index("require_inputs()", recovery_start)]
        current = recovery.index(
            'mount_state="$(read_tmpfs_mount_state'
        )
        checkpoint = recovery.index(
            '"$PRODUCER" publish-cleanup-checkpoint', current
        )
        unmount = recovery.index('umount "$staging"', checkpoint)
        post_unmount = recovery.index(
            '"$PRODUCER" verify-recovery-after-unmount', unmount
        )
        publication = recovery.index("publish_collectable_parent", unmount)
        removal = recovery.index('"$PRODUCER" remove-verified-recovery', publication)
        self.assertLess(current, checkpoint)
        self.assertLess(checkpoint, unmount)
        self.assertLess(unmount, post_unmount)
        self.assertLess(post_unmount, publication)
        self.assertLess(publication, removal)

    def test_cleanup_only_accepts_claim_bound_recovery_inputs_and_no_generation_inputs(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("--cleanup-only)", source)
        self.assertIn('mode="cleanup-only"', source)
        parser_start = source.index('if [[ $mode == "cleanup-only" ]]')
        parser = source[
            parser_start : source.index(
                'if [[ $mode != "produce" && $mode != "cleanup-only"', parser_start
            )
        ]
        execution_start = source.index("qualification_prefix()")
        cleanup_mode = source[
            source.index('if [[ $mode == "cleanup-only" ]]', execution_start) :
            source.index('if [[ $mode == "rehearsal" ]]', execution_start)
        ]
        for required in (
            "github_run_id",
            "github_run_attempt",
            "event_name",
            "dispatch_ref",
            "workflow_ref",
            "workflow_path",
            "head_sha",
            "head_authority_sha256",
            "claim_ref",
            "expected_tag_object_sha",
            "replica_ordinal",
            "outputs",
        ):
            self.assertIn(required, parser)
        for forbidden in (
            "--cas",
            "--launcher",
            "--result",
            "strategy_job_index",
            "strategy_job_total",
            "artifact_name",
            "systemd-run",
            "mount -t",
            "ATTEMPT-CONSUMED.json\" >",
        ):
            self.assertNotIn(forbidden, cleanup_mode)
        self.assertIn("verify_live_dispatch_claim", cleanup_mode)
        self.assertIn('"$PRODUCER" production-check', cleanup_mode)
        self.assertIn("initialise_production_recovery_identity", cleanup_mode)
        self.assertIn("recover_production_state", cleanup_mode)

    def test_cleanup_only_and_normal_exit_share_one_exact_recovery_state_machine(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        start = source.index("recover_production_state()")
        end = source.index("require_inputs()", start)
        recovery = source[start:end]
        qualification = recovery.index('stop_and_verify_unit "$qualification_unit"')
        production = recovery.index('stop_and_verify_unit "$production_unit"')
        preflight = recovery.index('stop_and_verify_unit "$preflight_unit"')
        absent = recovery.index('[[ ! -e $scratch && ! -L $scratch ]]')
        capture = recovery.index("read_tmpfs_mount_state", absent)
        checkpoint = recovery.index(
            '"$PRODUCER" publish-cleanup-checkpoint', capture
        )
        unmount = recovery.index('umount "$staging"', checkpoint)
        post_unmount = recovery.index(
            '"$PRODUCER" verify-recovery-after-unmount', unmount
        )
        publication = recovery.index("publish_collectable_parent", unmount)
        removal = recovery.index('"$PRODUCER" remove-verified-recovery', publication)
        self.assertLess(qualification, production)
        self.assertLess(production, preflight)
        self.assertLess(preflight, absent)
        self.assertLess(absent, capture)
        self.assertLess(capture, checkpoint)
        self.assertLess(checkpoint, unmount)
        self.assertLess(unmount, post_unmount)
        self.assertLess(post_unmount, publication)
        self.assertLess(publication, removal)
        self.assertIn('[[ $scratch == "$production_scratch" ]]', recovery)
        self.assertIn('[[ $expected_mount_source == "$recovery_stem" ]]', recovery)
        self.assertIn("already-clean", recovery)
        for forbidden in (
            "pkill",
            "killall",
            "list-units",
            "*service",
            "find /run",
            "mount -t",
            "systemd-run",
            "|| true",
        ):
            self.assertNotIn(forbidden, recovery)

    def test_cleanup_only_recreates_and_validates_ephemeral_recovery_root_before_absent_scratch_recovery(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        cleanup_host = source[
            source.index("require_cleanup_host()") :
            source.index("capture_tmpfs_mount_identity()")
        ]
        self.assertIn("install", cleanup_host)
        cleanup = source[
            source.index('if [[ $mode == "cleanup-only" ]]', source.index("qualification_prefix()")) :
            source.index('if [[ $mode == "rehearsal" ]]', source.index("qualification_prefix()"))
        ]
        initialise = cleanup.index("initialise_production_recovery_identity")
        root_parent = cleanup.index("require_root_recovery_parent", initialise)
        scratch = cleanup.index('scratch="$production_scratch"', root_parent)
        recover = cleanup.index("recover_production_state", scratch)
        self.assertLess(initialise, root_parent)
        self.assertLess(root_parent, scratch)
        self.assertLess(scratch, recover)

        normal_cleanup_start = source.rindex("cleanup()")
        normal_cleanup = source[
            normal_cleanup_start : source.index("trap cleanup EXIT", normal_cleanup_start)
        ]
        self.assertIn("recover_production_state", normal_cleanup)
        cleanup_mode = source[source.index('if [[ $mode == "cleanup-only" ]]') :]
        self.assertIn("recover_production_state", cleanup_mode)

    def test_cleanup_only_resumes_success_seal_without_produce_or_qualify(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        self.assertIn("resume_successful_parent()", source)
        start = source.index("resume_successful_parent()")
        end = source.index("require_recovery_output_parent()", start)
        resume = source[start:end]
        self.assertIn('"$PRODUCER" publish-and-seal-replica', resume)
        self.assertIn('git_repo cat-file tag "$tag_object_sha"', resume)
        self.assertIn("recheck_dispatch_claim_ref", resume)
        self.assertNotIn("systemd-run", resume)
        self.assertNotIn('"$PRODUCER" produce', resume)
        self.assertNotIn('"$PRODUCER" qualify', resume)
        self.assertNotIn("seal-replica-bundle", resume)

        recovery_start = source.index("recover_production_state()")
        recovery = source[
            recovery_start : source.index("require_inputs()", recovery_start)
        ]
        self.assertIn('output_recovery_state != "success-pending-seal"', recovery)
        self.assertGreaterEqual(
            recovery.count('[[ $output_recovery_state == "success-pending-seal" ]]'),
            2,
        )
        self.assertGreaterEqual(recovery.count("resume_successful_parent"), 2)
        for branch in recovery.split(
            '[[ $output_recovery_state == "success-pending-seal" ]]'
        )[1:]:
            self.assertLess(
                branch.index("resume_successful_parent"),
                branch.index("elif "),
            )

    def test_recovery_classifies_output_through_the_claim_bound_core_not_find(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        start = source.index("recover_production_state()")
        recovery = source[start : source.index("require_inputs()", start)]
        self.assertIn('"$PRODUCER" reconcile-output-state', recovery)
        self.assertIn('--repository-root "$ROOT"', recovery)
        self.assertIn('--parent "$outputs_parent"', recovery)
        self.assertIn('--parent-device "$parent_device"', recovery)
        self.assertIn('--parent-inode "$parent_inode"', recovery)
        self.assertIn('output_recovery_state="$', recovery)
        self.assertIn('[[ $output_recovery_state == "consumed" ]]', recovery)
        self.assertIn('[[ $output_recovery_state == "sealed" ]]', recovery)
        self.assertIn('[[ $output_recovery_state == "unconsumed" ]]', recovery)
        self.assertNotIn("verify_unconsumed_output_state", recovery)
        self.assertNotIn("find ", recovery)

    def test_pre_record_crashes_are_classified_before_unmount_and_discarded_by_core(
        self,
    ) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        start = source.index("recover_production_state()")
        recovery = source[start : source.index("require_inputs()", start)]
        output_state = recovery.index('"$PRODUCER" reconcile-output-state')
        record_state = recovery.index(
            '"$PRODUCER" reconcile-recovery-record-publication'
        )
        unmount = recovery.index('umount "$staging"', record_state)
        discard = recovery.index(
            '"$PRODUCER" discard-incomplete-recovery', unmount
        )
        checkpoint = recovery.index(
            '"$PRODUCER" publish-cleanup-checkpoint', record_state
        )
        self.assertLess(output_state, record_state)
        self.assertLess(record_state, unmount)
        self.assertLess(unmount, discard)
        self.assertLess(record_state, checkpoint)
        self.assertIn('[[ $output_recovery_state == "unconsumed" ]]', recovery)
        self.assertIn('[[ $record_recovery_state == "incomplete-no-record" ]]', recovery)
        self.assertIn('[[ $record_recovery_state == "record-ready" ]]', recovery)
        self.assertNotIn('rm -rf -- "$scratch"', recovery)

    def test_cleanup_preserves_the_primary_failure_and_reports_secondary_failures(self) -> None:
        source = WRAPPER.read_text(encoding="utf-8")
        starts = [
            match.start()
            for match in re.finditer(r"^\s*cleanup\(\) \{", source, re.MULTILINE)
        ]
        self.assertEqual(len(starts), 2)
        for index, start in enumerate(starts):
            with self.subTest(cleanup=index):
                block = source[start : source.index("trap cleanup EXIT", start)]
                self.assertIn("local primary_status=$?", block)
                self.assertIn("local cleanup_status=0", block)
                self.assertIn("trap - EXIT", block)
                self.assertIn('exit "$primary_status"', block)
                self.assertIn("cleanup also failed after primary status", block)
                self.assertNotIn("|| die", block)


if __name__ == "__main__":
    unittest.main()
