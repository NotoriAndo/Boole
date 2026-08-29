#!/usr/bin/env python3
"""RED contract for the launcher-v2, no-image arm64 staging preflight.

The launcher-v2 bytes are sealed, but the current boot builder is intentionally
still a launcher-v1 consumer.  This gate describes the next *free* step: a new
predecessor-pinned builder projection and a repeatable staging preflight.  The
preflight may assemble and measure the real tree; it may not make an initrd, a
disk image, an attempt marker, or an output directory for production.

The arm64 workflow is the only place that can make the exact launcher-v2 ELF.
Unit tests therefore prove the local plumbing with stand-in bytes and leave the
exact ELF success case to that workflow.  They still require the local path to
validate the sealed size and digest and to forward the validated bytes to the
same assembler object a later, separately authorised producer would consume.
"""
from __future__ import annotations

import ast
import copy
import hashlib
import importlib
import inspect
import json
import pathlib
import stat
import tempfile
import unittest
from unittest import mock


REPO = pathlib.Path(__file__).resolve().parents[1]
PREREGISTRATION = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json"
)
PREREGISTRATION_SHA256 = (
    "bb51f61b044b9ff651282860eb8645dc97e9122bc446cf65f2489bfefbd73173"
)
BUILDER_V3 = REPO / "scripts/native_shadow_rootfs_builder_boot_arm64_v3.py"
BUILDER_V4 = REPO / "scripts/native_shadow_rootfs_builder_boot_arm64_v4.py"
PREFLIGHT_MODULE = REPO / "scripts/native_shadow_launcher_v2_image_preflight_arm64_v1.py"
HISTORICAL_PRODUCER = REPO / "scripts/native_shadow_successor_produce_phase_arm64_v2.py"
HISTORICAL_WORKFLOW = REPO / ".github/workflows/native-shadow-successor-produce-arm64.yml"
HISTORICAL_PRODUCER_SHA256 = (
    "1c1b99257aa5f2d3f144387f72903fc167d6ba8c8b71a74c1b9a6c845073c1a8"
)
HISTORICAL_WORKFLOW_SHA256 = (
    "a6ff2019a9e8f95580ebcb82e32d3a12f1a0397bb25912478716772683601b61"
)

V1_LAUNCHER_SHA256 = (
    "11b5d1cf1728aff271c589129292bcd8ad07a1d928652d2435b1c9010f73c434"
)
V1_LAUNCHER_SIZE = 2_006_632
V2_LAUNCHER_SHA256 = (
    "53412188cec4488cf694450548991607c66e9281ccf54e6b462d34b3a345decd"
)
V2_LAUNCHER_SIZE = 2_025_192
LAUNCHER_DELTA = 18_560
EXPECTED_ENTRIES = 17_676
EXPECTED_PAYLOAD_BYTES = 1_773_475_059
EXPECTED_LARGEST_FILE_BYTES = 160_096_808
EXPECTED_LIMITS = {
    "maxEntries": 200_000,
    "maxFileBytes": 536_870_912,
    "maxTotalBytes": 2_147_483_648,
}
LAUNCHER_GUEST_PATH = "/usr/libexec/boole/boole-native-shadow-launcher"
LAUNCHER_ENTRY_PATH = "usr/libexec/boole/boole-native-shadow-launcher"


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def preregistration() -> dict:
    return json.loads(PREREGISTRATION.read_text(encoding="utf-8"))


def require_module(path: pathlib.Path, dotted: str):
    if not path.is_file():
        raise AssertionError(f"the S2-A module does not exist yet: {path}")
    return importlib.import_module(dotted)


def builder_v4():
    return require_module(
        BUILDER_V4, "scripts.native_shadow_rootfs_builder_boot_arm64_v4"
    )


def preflight_module():
    return require_module(
        PREFLIGHT_MODULE,
        "scripts.native_shadow_launcher_v2_image_preflight_arm64_v1",
    )


class FrozenInputTests(unittest.TestCase):
    def test_preregistration_bytes_are_pinned_before_implementation(self) -> None:
        self.assertEqual(sha256(PREREGISTRATION), PREREGISTRATION_SHA256)
        raw = PREREGISTRATION.read_bytes()
        self.assertEqual(
            raw,
            (json.dumps(json.loads(raw), sort_keys=True, indent=2) + "\n").encode(),
        )

    def test_new_builder_pins_the_exact_v3_predecessor(self) -> None:
        mod = builder_v4()
        self.assertEqual(mod.BOOT_V3.resolve(), BUILDER_V3.resolve())
        self.assertEqual(mod.BOOT_V3_SHA256, sha256(BUILDER_V3))

    def test_preflight_pins_and_reconstructs_the_s1_record(self) -> None:
        preflight = preflight_module()
        self.assertEqual(
            pathlib.Path(preflight.PREREGISTRATION_PATH).resolve(),
            PREREGISTRATION.resolve(),
        )
        self.assertEqual(preflight.PREREGISTRATION_SHA256, PREREGISTRATION_SHA256)
        self.assertEqual(preflight.load_preregistration(), preregistration())

    def test_exhausted_historical_producer_and_workflow_stay_byte_exact(self) -> None:
        self.assertEqual(sha256(HISTORICAL_PRODUCER), HISTORICAL_PRODUCER_SHA256)
        self.assertEqual(sha256(HISTORICAL_WORKFLOW), HISTORICAL_WORKFLOW_SHA256)
        value = preregistration()["generation"]
        self.assertTrue(value["historicalProducerAndWorkflowStayByteUnchanged"])
        self.assertTrue(value["newGenerationFilesOnly"])

    def test_every_preregistered_binding_is_rechecked_before_use(self) -> None:
        mod = preflight_module()
        with tempfile.TemporaryDirectory(prefix="boole-s2-bindings.") as scratch:
            root = pathlib.Path(scratch)
            path = root / "bound.txt"
            path.write_bytes(b"sealed")
            record = {
                "bindings": [
                    {
                        "path": "bound.txt",
                        "sha256": hashlib.sha256(b"sealed").hexdigest(),
                        "sizeBytes": 6,
                    }
                ]
            }
            self.assertEqual(
                mod.verify_bound_inputs(record, root), record["bindings"]
            )
            path.write_bytes(b"drifted")
            with self.assertRaises(mod.LauncherV2PreflightError):
                mod.verify_bound_inputs(record, root)
            record["bindings"][0]["path"] = "../bound.txt"
            with self.assertRaises(mod.LauncherV2PreflightError):
                mod.verify_bound_inputs(record, root)


class BuilderProjectionTests(unittest.TestCase):
    def test_projection_is_derived_from_the_sealed_record(self) -> None:
        mod = preflight_module()
        expected = preregistration()["expectedProjection"]
        self.assertEqual(mod.expected_projection(), expected)
        self.assertEqual(expected["launcherSizeDeltaBytes"], LAUNCHER_DELTA)
        self.assertEqual(expected["withLauncherV2"]["entries"], EXPECTED_ENTRIES)
        self.assertEqual(
            expected["withLauncherV2"]["payloadBytes"], EXPECTED_PAYLOAD_BYTES
        )
        self.assertEqual(
            expected["withLauncherV2"]["largestFileBytes"],
            EXPECTED_LARGEST_FILE_BYTES,
        )
        self.assertEqual(expected["limits"], EXPECTED_LIMITS)

    def test_successor_launcher_identity_and_metadata_are_exact(self) -> None:
        mod = builder_v4()
        identity = {
            "guestLogicalPath": mod.__getattr__("LAUNCHER_GUEST_PATH"),
            "sha256": mod.__getattr__("LAUNCHER_SHA256"),
            "sizeBytes": mod.__getattr__("LAUNCHER_SIZE_BYTES"),
        }
        self.assertEqual(
            identity,
            {
                "guestLogicalPath": LAUNCHER_GUEST_PATH,
                "sha256": V2_LAUNCHER_SHA256,
                "sizeBytes": V2_LAUNCHER_SIZE,
            },
        )
        stand_in = bytes(V2_LAUNCHER_SIZE)
        with mock.patch.dict(
            mod._IMPL,
            {"sha256_hex": lambda observed: V2_LAUNCHER_SHA256},
        ):
            entry = mod.__getattr__("launcher_entry")(stand_in)
        self.assertEqual(
            {key: value for key, value in entry.items() if key != "raw"},
            {
                "gid": 0,
                "kind": "file",
                "mode": 0o755,
                "path": LAUNCHER_ENTRY_PATH,
                "uid": 0,
            },
        )

    def test_historical_v1_identity_is_not_an_accepted_successor(self) -> None:
        mod = builder_v4()
        historical = preregistration()["expectedProjection"]["historicalLauncher"]
        self.assertEqual(historical["sha256"], V1_LAUNCHER_SHA256)
        self.assertEqual(historical["sizeBytes"], V1_LAUNCHER_SIZE)
        with mock.patch.dict(
            mod._IMPL,
            {"sha256_hex": lambda observed: V1_LAUNCHER_SHA256},
        ):
            with self.assertRaises(mod.RootfsBuildError):
                mod.__getattr__("launcher_entry")(
                    b"v1-sized-placeholder".ljust(V1_LAUNCHER_SIZE, b"\0")
                )

    def test_wrong_v2_size_or_digest_is_fail_closed(self) -> None:
        mod = builder_v4()
        for raw in (b"", b"wrong", b"\0" * V2_LAUNCHER_SIZE):
            with self.subTest(size=len(raw)):
                with self.assertRaises(mod.RootfsBuildError):
                    mod.__getattr__("launcher_entry")(raw)

    def test_exact_projection_fits_all_three_frozen_limits(self) -> None:
        mod = preflight_module()
        totals = {
            "entries": EXPECTED_ENTRIES,
            "largestFileBytes": EXPECTED_LARGEST_FILE_BYTES,
            "payloadBytes": EXPECTED_PAYLOAD_BYTES,
        }
        self.assertEqual(mod.require_projected_limits(totals), totals)
        self.assertEqual(mod.require_expected_totals(totals), totals)
        for key in totals:
            changed = dict(totals)
            changed[key] -= 1
            with self.subTest(key=key, direction="below-exact-projection"):
                with self.assertRaises(mod.LauncherV2PreflightError):
                    mod.require_expected_totals(changed)
        for key, limit_key in (
            ("entries", "maxEntries"),
            ("largestFileBytes", "maxFileBytes"),
            ("payloadBytes", "maxTotalBytes"),
        ):
            changed = dict(totals)
            changed[key] = EXPECTED_LIMITS[limit_key] + 1
            with self.subTest(key=key):
                with self.assertRaises(mod.LauncherV2PreflightError):
                    mod.require_projected_limits(changed)


class PreflightAssemblyTests(unittest.TestCase):
    def test_preflight_exports_the_builder_assembler_itself(self) -> None:
        builder = builder_v4()
        mod = preflight_module()
        self.assertIs(mod.ASSEMBLER, builder.materialize_staging_tree)

    def test_validated_launcher_bytes_are_forwarded_to_that_assembler(self) -> None:
        mod = preflight_module()
        raw = b"\x7fELF launcher-v2 arm64 stand-in"
        nested = {"runtime": {"path": "runtime", "kind": "directory"}}
        captured = {}

        def assembler(validated, repository_root, artifact_store, **kwargs):
            captured.update(kwargs)
            return {
                LAUNCHER_ENTRY_PATH: {
                    "path": LAUNCHER_ENTRY_PATH,
                    "kind": "file",
                    "mode": 0o755,
                    "uid": 0,
                    "gid": 0,
                    "raw": kwargs["launcher_binary"],
                }
            }

        with mock.patch.object(mod, "ASSEMBLER", assembler):
            entries = mod.assemble(
                validated={"lock": {}},
                repository_root=REPO,
                artifact_store=REPO,
                launcher_binary=raw,
                nested_tree=nested,
            )
        self.assertIs(captured["launcher_binary"], raw)
        self.assertIs(captured["nested_tree"], nested)
        self.assertEqual(entries[LAUNCHER_ENTRY_PATH]["raw"], raw)

    def test_baseline_is_derived_without_running_the_large_assembler_twice(self) -> None:
        mod = preflight_module()
        successor = {
            "usr": {"path": "usr", "kind": "directory"},
            "usr/libexec": {"path": "usr/libexec", "kind": "directory"},
            "usr/libexec/boole": {
                "path": "usr/libexec/boole",
                "kind": "directory",
            },
            LAUNCHER_ENTRY_PATH: {
                "path": LAUNCHER_ENTRY_PATH,
                "kind": "file",
                "mode": 0o755,
                "uid": 0,
                "gid": 0,
                "raw": b"launcher",
            },
        }
        baseline = mod._baseline_without_launcher(successor)
        self.assertEqual(set(baseline), {"usr", "usr/libexec"})
        self.assertEqual(set(successor), {
            "usr",
            "usr/libexec",
            "usr/libexec/boole",
            LAUNCHER_ENTRY_PATH,
        })

    def test_written_launcher_bytes_and_unix_metadata_are_independently_required(self) -> None:
        mod = preflight_module()
        expected = {
            "gid": 0,
            "kind": "file",
            "mode": 0o755,
            "path": LAUNCHER_ENTRY_PATH,
            "uid": 0,
        }
        info = mock.Mock(st_mode=stat.S_IFREG | 0o755, st_uid=0, st_gid=0)
        path = mock.Mock()
        path.read_bytes.return_value = b"launcher"
        path.lstat.return_value = info
        self.assertEqual(
            mod.require_launcher_on_disk(path, b"launcher", expected), expected
        )
        info.st_gid = 1
        with self.assertRaises(mod.LauncherV2PreflightError):
            mod.require_launcher_on_disk(path, b"launcher", expected)

    def test_preflight_public_api_has_no_production_output_argument(self) -> None:
        mod = preflight_module()
        parameters = inspect.signature(mod.preflight).parameters
        for forbidden in ("outputs", "output_dir", "kernel", "initrd", "root_disk"):
            self.assertNotIn(forbidden, parameters)
        self.assertIn("result_path", parameters)
        self.assertIn("launcher_path", parameters)

class NoImageBoundaryTests(unittest.TestCase):
    IMAGE_MODULE_FRAGMENTS = (
        "native_shadow_boot_kernel_extract",
        "native_shadow_boot_initrd",
        "native_shadow_boot_root_disk",
        "native_shadow_boot_image_produce",
        "native_shadow_successor_produce",
    )

    def test_preflight_import_graph_contains_no_image_or_production_module(self) -> None:
        source = PREFLIGHT_MODULE.read_text(encoding="utf-8")
        tree = ast.parse(source)
        imported = []
        for node in ast.walk(tree):
            if isinstance(node, ast.Import):
                imported.extend(alias.name for alias in node.names)
            elif isinstance(node, ast.ImportFrom):
                imported.append(node.module or "")
                imported.extend(alias.name for alias in node.names)
        joined = "\n".join(imported)
        for fragment in self.IMAGE_MODULE_FRAGMENTS:
            with self.subTest(fragment=fragment):
                self.assertNotIn(fragment, joined)

    def test_preflight_has_no_image_or_activation_claim(self) -> None:
        mod = preflight_module()
        self.assertFalse(mod.IMAGE_PRODUCED_CLAIM)
        self.assertFalse(mod.BOOTABLE_CLAIM)
        self.assertFalse(mod.ACTIVATION_ALLOWED)
        self.assertEqual(mod.ALLOWED_IMAGE_TOOLS, frozenset())
        self.assertEqual(
            set(mod.FORBIDDEN_OUTPUT_NAMES),
            {"ATTEMPT-CONSUMED.json", "guest-kernel", "guest-initrd", "guest-root-disk"},
        )

    def test_preflight_call_graph_cannot_reach_a_marker_or_image_step(self) -> None:
        mod = preflight_module()
        # This is a public self-audit, not a comment: future edits must still be
        # rejected before any tree is assembled.
        mod.assert_no_image_path()


class CanonicalRepeatabilityTests(unittest.TestCase):
    def test_actual_result_builder_is_path_and_clock_independent(self) -> None:
        mod = preflight_module()
        prereg = preregistration()
        totals = {
            "entries": EXPECTED_ENTRIES,
            "largestFileBytes": EXPECTED_LARGEST_FILE_BYTES,
            "payloadBytes": EXPECTED_PAYLOAD_BYTES,
        }
        kwargs = {
            "preregistration": prereg,
            "computed": totals,
            "walked": totals,
            "launcher_binary": b"fixed launcher bytes",
            "baseline_totals": prereg["expectedProjection"]["withoutLauncher"],
            "nested_manifest": {"guestPath": "/fixed", "sha256": "0" * 64, "sizeBytes": 1},
            "bound_inputs": [],
            "gpgv": pathlib.Path("/usr/bin/true"),
            "zstd": pathlib.Path("/usr/bin/true"),
            "repository_root": REPO,
        }
        first = mod.build_result_document(**kwargs)
        second = mod.build_result_document(**kwargs)
        self.assertEqual(mod.canonical_json(first), mod.canonical_json(second))
        raw = mod.canonical_json(first)
        self.assertNotIn(b"/tmp/", raw)
        self.assertNotIn(b"runner", raw.lower())
        self.assertNotIn(b"timestamp", raw.lower())

    def test_two_fresh_result_paths_receive_identical_canonical_bytes(self) -> None:
        mod = preflight_module()
        document = {
            "activationAllowed": False,
            "imageProduced": False,
            "launcherSha256": V2_LAUNCHER_SHA256,
            "schema": "boole.native-shadow.launcher-v2-image-preflight.arm64.v1",
            "totals": {
                "entries": EXPECTED_ENTRIES,
                "largestFileBytes": EXPECTED_LARGEST_FILE_BYTES,
                "payloadBytes": EXPECTED_PAYLOAD_BYTES,
            },
        }
        with tempfile.TemporaryDirectory(prefix="boole-launcher-v2-preflight.") as scratch:
            root = pathlib.Path(scratch)
            first = root / "first.json"
            second = root / "second.json"
            mod.write_result_once(first, document)
            mod.write_result_once(second, document)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            self.assertEqual(first.read_bytes(), mod.canonical_json(document))

    def test_an_existing_result_is_never_replaced_even_by_identical_bytes(self) -> None:
        mod = preflight_module()
        document = {"schema": "stand-in", "status": "PASS"}
        with tempfile.TemporaryDirectory(prefix="boole-launcher-v2-preflight.") as scratch:
            path = pathlib.Path(scratch) / "result.json"
            mod.write_result_once(path, document)
            before = path.read_bytes()
            with self.assertRaises(mod.LauncherV2PreflightError):
                mod.write_result_once(path, document)
            self.assertEqual(path.read_bytes(), before)

    def test_a_dangling_result_symlink_is_also_an_existing_name(self) -> None:
        mod = preflight_module()
        with tempfile.TemporaryDirectory(prefix="boole-launcher-v2-preflight.") as scratch:
            path = pathlib.Path(scratch) / "result.json"
            path.symlink_to(path.parent / "missing")
            with self.assertRaises(mod.LauncherV2PreflightError):
                mod.write_result_once(path, {"schema": "stand-in"})
            self.assertTrue(path.is_symlink())


class ResultVerificationTests(unittest.TestCase):
    def valid_document(self):
        mod = preflight_module()
        prereg = preregistration()
        baseline = prereg["expectedProjection"]["withoutLauncher"]
        totals = dict(baseline)
        totals.update(
            {
                "byKind": {"directory": 1737, "file": 15102, "symlink": 837},
                "entries": EXPECTED_ENTRIES,
                "pathManifestSha256": "a" * 64,
                "payloadBytes": EXPECTED_PAYLOAD_BYTES,
            }
        )
        measurement_record = json.loads(
            (
                REPO
                / "native/containment/native-shadow-boot-staging-tree-measurement-arm64-v1.json"
            ).read_text(encoding="utf-8")
        )
        tool = pathlib.Path("/usr/bin/true")
        document = mod.build_result_document(
            preregistration=prereg,
            computed=totals,
            walked=totals,
            launcher_binary=b"x" * V2_LAUNCHER_SIZE,
            baseline_totals=baseline,
            nested_manifest=measurement_record["nestedContentManifest"],
            bound_inputs=mod.verify_bound_inputs(prereg, REPO),
            gpgv=tool,
            zstd=tool,
            repository_root=REPO,
        )
        document["launcher"] = prereg["expectedProjection"]["successorLauncher"]
        return document

    def test_full_result_is_accepted_by_the_read_only_consumer(self) -> None:
        mod = preflight_module()
        document = self.valid_document()
        self.assertEqual(
            mod.verify_result_document(
                document,
                repository_root=REPO,
                gpgv=pathlib.Path("/usr/bin/true"),
                zstd=pathlib.Path("/usr/bin/true"),
            ),
            document,
        )

    def test_four_claims_without_the_evidence_are_rejected(self) -> None:
        mod = preflight_module()
        truncated = {
            "activationAllowed": False,
            "bootableClaim": False,
            "imageProduced": False,
            "status": "PASS-NO-IMAGE-PRODUCED",
        }
        with self.assertRaises(mod.LauncherV2PreflightError):
            mod.verify_result_document(
                truncated,
                repository_root=REPO,
                gpgv=pathlib.Path("/usr/bin/true"),
                zstd=pathlib.Path("/usr/bin/true"),
            )

    def test_result_file_must_be_canonical_before_it_is_consumed(self) -> None:
        mod = preflight_module()
        document = self.valid_document()
        with tempfile.TemporaryDirectory(prefix="boole-s2-result-consumer.") as scratch:
            path = pathlib.Path(scratch) / "result.json"
            path.write_text(json.dumps(document), encoding="utf-8")
            with self.assertRaises(mod.LauncherV2PreflightError):
                mod.verify_result_file(
                    path,
                    repository_root=REPO,
                    gpgv=pathlib.Path("/usr/bin/true"),
                    zstd=pathlib.Path("/usr/bin/true"),
                )
            path.write_bytes(mod.canonical_json(document))
            self.assertEqual(
                mod.verify_result_file(
                    path,
                    repository_root=REPO,
                    gpgv=pathlib.Path("/usr/bin/true"),
                    zstd=pathlib.Path("/usr/bin/true"),
                ),
                document,
            )

    def test_identity_measurement_binding_and_provenance_tampering_are_rejected(self) -> None:
        mod = preflight_module()
        original = self.valid_document()
        mutations = (
            (
                "false-as-zero",
                lambda value: value.__setitem__("activationAllowed", 0),
            ),
            (
                "true-as-one",
                lambda value: value.__setitem__("repeatable", 1),
            ),
            (
                "authority-false-as-zero",
                lambda value: value["authorisations"].__setitem__(
                    "bootAuthorised", 0
                ),
            ),
            (
                "measurement-zero-as-false",
                lambda value: (
                    value["builderInternal"].__setitem__("duplicatePaths", False),
                    value["independentTraversal"].__setitem__(
                        "duplicatePaths", False
                    ),
                ),
            ),
            ("launcher", lambda value: value["launcher"].__setitem__("sha256", "0" * 64)),
            (
                "measurement",
                lambda value: value["builderInternal"].__setitem__(
                    "payloadBytes", value["builderInternal"]["payloadBytes"] - 1
                ),
            ),
            (
                "binding",
                lambda value: value["boundInputs"][0].__setitem__("sha256", "0" * 64),
            ),
            (
                "provenance",
                lambda value: value["provenance"]["repositoryFiles"][0].__setitem__(
                    "sha256", "0" * 64
                ),
            ),
        )
        for label, mutate in mutations:
            changed = copy.deepcopy(original)
            mutate(changed)
            with self.subTest(label=label):
                with self.assertRaises(mod.LauncherV2PreflightError):
                    mod.verify_result_document(
                        changed,
                        repository_root=REPO,
                        gpgv=pathlib.Path("/usr/bin/true"),
                        zstd=pathlib.Path("/usr/bin/true"),
                    )

if __name__ == "__main__":
    unittest.main()
