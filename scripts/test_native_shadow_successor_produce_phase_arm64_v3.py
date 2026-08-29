#!/usr/bin/env python3
"""Contract tests for the authority-zero launcher-v2 successor producer core."""

from __future__ import annotations

import hashlib
import inspect
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from scripts import native_shadow_successor_produce_phase_arm64_v3 as producer
from scripts import native_shadow_rootfs_builder_boot_arm64_v4 as builder_v4


class SuccessorProducerV3ContractTests(unittest.TestCase):
    def test_preregistration_is_canonical_and_grants_no_authority(self) -> None:
        document = producer.load_preregistration()

        self.assertEqual(
            document["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-"
            "preregistration.arm64.v1",
        )
        self.assertEqual(
            document["status"],
            "PRE-REGISTERED-NO-IMAGE-PRODUCTION-AUTHORITY",
        )
        self.assertTrue(
            all(
                value is False
                for value in document["authorisations"].values()
                if isinstance(value, bool)
            )
        )
        self.assertEqual(document["authorisations"]["imageProductionRunsAllowed"], 0)
        self.assertEqual(document["runs"]["imageProductionsAllowed"], 0)
        self.assertEqual(document["runs"]["imageProductionsPerformed"], 0)

    def test_import_closure_correction_is_fixed_and_grants_no_authority(self) -> None:
        preregistration = producer.load_preregistration()
        correction = producer.load_import_closure_correction(preregistration)

        self.assertEqual(
            correction["status"],
            "CORRECTED-BEFORE-REHEARSAL-NO-IMAGE-PRODUCTION-AUTHORITY",
        )
        self.assertEqual(correction["effectiveBinding"]["predecessorBindings"], 23)
        self.assertEqual(correction["effectiveBinding"]["addedMissingBindings"], 18)
        self.assertEqual(correction["effectiveBinding"]["effectiveUniqueBindings"], 41)
        self.assertEqual(correction["authorisations"], preregistration["authorisations"])
        self.assertEqual(correction["runs"], preregistration["runs"])

    def test_preregistration_tampering_and_symlink_substitution_fail_closed(self) -> None:
        live = (
            producer.REPOSITORY_ROOT / producer.PREREGISTRATION_RELATIVE
        ).read_bytes()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            target = root / producer.PREREGISTRATION_RELATIVE
            target.parent.mkdir(parents=True)
            target.write_bytes(live)
            self.assertEqual(
                producer.load_preregistration(root)["schema"],
                producer.PREREGISTRATION_SCHEMA,
            )

            tampered = bytearray(live)
            tampered[-2] ^= 1
            target.write_bytes(tampered)
            with self.assertRaisesRegex(
                producer.SuccessorProducerV3Error,
                "digest differs",
            ):
                producer.load_preregistration(root)

            target.unlink()
            target.symlink_to(
                producer.REPOSITORY_ROOT / producer.PREREGISTRATION_RELATIVE
            )
            with self.assertRaisesRegex(
                producer.SuccessorProducerV3Error,
                "non-symlink",
            ):
                producer.load_preregistration(root)

    def test_all_forty_one_effective_inputs_are_reverified_by_identity(self) -> None:
        document = producer.load_preregistration()
        correction = producer.load_import_closure_correction(document)

        observed = producer.verify_bound_inputs(document, correction)

        expected = sorted(
            (
                {
                    "path": row["path"],
                    "sha256": row["sha256"],
                    "sizeBytes": row["sizeBytes"],
                }
                for row in document["bindings"] + correction["addedBindings"]
            ),
            key=lambda row: row["path"].encode("utf-8"),
        )
        self.assertEqual(len(observed), 41)
        self.assertEqual(observed, expected)

    def test_import_correction_tampering_and_wrong_predecessor_fail_closed(self) -> None:
        preregistration = producer.load_preregistration()
        live = (
            producer.REPOSITORY_ROOT / producer.IMPORT_CORRECTION_RELATIVE
        ).read_bytes()
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            predecessor = root / producer.PREREGISTRATION_RELATIVE
            predecessor.parent.mkdir(parents=True)
            predecessor.write_bytes(
                (producer.REPOSITORY_ROOT / producer.PREREGISTRATION_RELATIVE).read_bytes()
            )
            target = root / producer.IMPORT_CORRECTION_RELATIVE
            target.write_bytes(live)
            loaded_predecessor = producer.load_preregistration(root)
            self.assertEqual(
                producer.load_import_closure_correction(loaded_predecessor, root)[
                    "effectiveBinding"
                ]["effectiveUniqueBindings"],
                41,
            )

            tampered = bytearray(live)
            tampered[-2] ^= 1
            target.write_bytes(tampered)
            with self.assertRaisesRegex(producer.SuccessorProducerV3Error, "correction.*digest"):
                producer.load_import_closure_correction(loaded_predecessor, root)

            target.write_bytes(live)
            wrong = json.loads(json.dumps(loaded_predecessor))
            wrong["bindings"][0]["sha256"] = "0" * 64
            with self.assertRaisesRegex(producer.SuccessorProducerV3Error, "predecessor"):
                producer.load_import_closure_correction(wrong, root)

    def test_prepare_staging_uses_the_fixed_builder_and_measurer(self) -> None:
        expected = producer.load_preregistration()["expectedPreflight"]["measurement"]
        entries = {
            "tiny": {
                "gid": 0,
                "kind": "file",
                "mode": 0o444,
                "raw": b"x",
                "uid": 0,
            }
        }
        calls = []

        def assemble(validated, repository_root, artifact_store, **kwargs):
            calls.append(
                (
                    "assemble",
                    validated,
                    repository_root,
                    artifact_store,
                    kwargs,
                )
            )
            return entries

        def measure(observed):
            calls.append(("measure", observed))
            self.assertIs(observed, entries)
            return dict(expected)

        with mock.patch.object(
            builder_v4, "materialize_staging_tree", side_effect=assemble
        ), mock.patch.object(
            producer.staging_measure, "builder_totals", side_effect=measure
        ):
            prepared = producer.prepare_staging(
                validated={"sealed": True},
                repository_root=Path("/repo"),
                artifact_store=Path("/cas"),
                launcher_binary=b"launcher-v2",
                nested_tree={"nested": {"kind": "directory"}},
            )

        self.assertIs(prepared.entries, entries)
        self.assertEqual(prepared.measurement, expected)
        self.assertEqual([row[0] for row in calls], ["assemble", "measure"])
        self.assertEqual(calls[0][1], {"sealed": True})
        self.assertEqual(calls[0][2], Path("/repo"))
        self.assertEqual(calls[0][3], Path("/cas"))
        self.assertEqual(calls[0][4]["launcher_binary"], b"launcher-v2")
        self.assertEqual(
            calls[0][4]["nested_tree"], {"nested": {"kind": "directory"}}
        )

    def test_public_orchestration_exposes_no_assembler_or_measurer_override(self) -> None:
        self.assertNotIn("ports", inspect.signature(producer.prepare_staging).parameters)
        self.assertNotIn("ports", inspect.signature(producer.run_free_rehearsal).parameters)
        self.assertNotIn(
            "assembly_ports", inspect.signature(producer.production_entry).parameters
        )

    def test_prepare_staging_rejects_even_one_measurement_drift(self) -> None:
        expected = producer.load_preregistration()["expectedPreflight"]["measurement"]
        drifted = dict(expected)
        drifted["entries"] += 1
        with mock.patch.object(
            builder_v4, "materialize_staging_tree", return_value={}
        ), mock.patch.object(
            producer.staging_measure, "builder_totals", return_value=drifted
        ):
            with self.assertRaisesRegex(
                producer.SuccessorProducerV3Error,
                "differs from the S2 preflight contract",
            ):
                producer.prepare_staging(
                    validated={},
                    repository_root=Path("/repo"),
                    artifact_store=Path("/cas"),
                    launcher_binary=b"launcher-v2",
                    nested_tree={},
                )

    def test_free_rehearsal_returns_one_canonical_json_without_image_effects(self) -> None:
        preregistration = producer.load_preregistration()
        expected = preregistration["expectedPreflight"]["measurement"]
        calls = []

        def assemble(*args, **kwargs):
            calls.append("assemble")
            return {"tiny": {"kind": "file", "raw": b"x"}}

        def measure(entries):
            calls.append("measure")
            return dict(expected)

        with tempfile.TemporaryDirectory() as temporary:
            with mock.patch.object(
                builder_v4, "materialize_staging_tree", side_effect=assemble
            ), mock.patch.object(
                producer.staging_measure, "builder_totals", side_effect=measure
            ):
                raw = producer.run_free_rehearsal(
                    validated={"sealed": True},
                    repository_root=Path("/repo"),
                    artifact_store=Path("/cas"),
                    launcher_binary=b"launcher-v2",
                    nested_tree={},
                    scratch_root=Path(temporary),
                )

        document = json.loads(raw.decode("utf-8"))
        self.assertEqual(raw, producer.canonical_json(document))
        self.assertEqual(calls, ["assemble", "measure"])
        self.assertEqual(document["status"], "PASS-NO-IMAGE-PRODUCED")
        self.assertEqual(document["measurement"], expected)
        self.assertEqual(len(document["boundInputs"]), 41)
        self.assertEqual(
            document["importClosureCorrectionSha256"],
            producer.IMPORT_CORRECTION_SHA256,
        )
        self.assertEqual(
            document["effects"],
            {
                "allowedArtifact": "one canonical JSON result only",
                "allowedImageTools": [],
                "artifactMemberCount": 1,
                "attemptMarkersCreated": 0,
                "forbiddenOutputNames": [
                    "ATTEMPT-CONSUMED.json",
                    "guest-kernel",
                    "guest-initrd",
                    "guest-root-disk",
                ],
                "imageEffectCalls": 0,
                "imageFilesCreated": 0,
                "productionOutputDirectoriesCreated": 0,
                "productionOutputsCreated": 0,
                "scratchSnapshotSha256": (
                    "37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570"
                ),
                "scratchTreeUnchanged": True,
            },
        )
        self.assertFalse(document["imageProduced"])
        self.assertFalse(document["bootableClaim"])
        self.assertFalse(document["activationAllowed"])

    def test_free_rehearsal_is_byte_repeatable(self) -> None:
        expected = producer.load_preregistration()["expectedPreflight"]["measurement"]
        with tempfile.TemporaryDirectory() as temporary:
            with mock.patch.object(
                builder_v4,
                "materialize_staging_tree",
                return_value={"tiny": {"kind": "file", "raw": b"x"}},
            ), mock.patch.object(
                producer.staging_measure, "builder_totals", return_value=dict(expected)
            ):
                arguments = {
                    "validated": {"sealed": True},
                    "repository_root": Path("/repo"),
                    "artifact_store": Path("/cas"),
                    "launcher_binary": b"launcher-v2",
                    "nested_tree": {},
                    "scratch_root": Path(temporary),
                }
                first = producer.run_free_rehearsal(**arguments)
                second = producer.run_free_rehearsal(**arguments)

        self.assertEqual(first, second)

    def test_free_rehearsal_rejects_any_scratch_effect(self) -> None:
        expected = producer.load_preregistration()["expectedPreflight"]["measurement"]
        with tempfile.TemporaryDirectory() as temporary:
            scratch = Path(temporary)

            def assemble(*args, **kwargs):
                (scratch / "guest-root-disk").write_bytes(b"forbidden")
                return {"tiny": {"kind": "file", "raw": b"x"}}

            with mock.patch.object(
                builder_v4, "materialize_staging_tree", side_effect=assemble
            ), mock.patch.object(
                producer.staging_measure, "builder_totals", return_value=dict(expected)
            ):
                with self.assertRaisesRegex(
                    producer.SuccessorProducerV3Error,
                    "scratch tree changed",
                ):
                    producer.run_free_rehearsal(
                        validated={"sealed": True},
                        repository_root=Path("/repo"),
                        artifact_store=Path("/cas"),
                        launcher_binary=b"launcher-v2",
                        nested_tree={},
                        scratch_root=scratch,
                    )

    def test_authorityless_production_refuses_before_every_effect(self) -> None:
        calls = []

        def forbidden(*args, **kwargs):
            calls.append((args, kwargs))
            raise AssertionError("an authorityless production effect was reached")

        production_effects = producer.ProductionEffects(
            create_output_directory=forbidden,
            write_attempt_marker=forbidden,
            produce_images=forbidden,
        )

        with self.assertRaisesRegex(
            producer.SuccessorProducerV3Error,
            "future production chain is absent",
        ):
            producer.production_entry(
                validated={"sealed": True},
                repository_root=Path("/repo"),
                artifact_store=Path("/cas"),
                launcher_binary=b"launcher-v2",
                nested_tree={},
                production_effects=production_effects,
            )

        self.assertEqual(calls, [])

    def test_generation_has_no_historical_or_override_gateway(self) -> None:
        source = Path(producer.__file__).read_text(encoding="utf-8")
        self.assertNotIn("native_shadow_successor_produce_phase_arm64_v2", source)
        self.assertNotIn("subprocess", source)
        self.assertNotIn("os.environ", source)
        parameters = set(inspect.signature(producer.production_entry).parameters)
        for forbidden in (
            "authority_path",
            "fingerprint_path",
            "preregistration_path",
            "assembler_path",
        ):
            self.assertNotIn(forbidden, parameters)
        production_source = inspect.getsource(producer.production_entry)
        self.assertIn("prepare_staging(", production_source)
        self.assertLess(
            production_source.index("prepare_staging("),
            production_source.index("create_output_directory("),
        )
        self.assertLess(
            production_source.index("create_output_directory("),
            production_source.index("write_attempt_marker("),
        )
        self.assertLess(
            production_source.index("write_attempt_marker("),
            production_source.index("produce_images("),
        )

    def test_future_chain_paths_are_fixed_by_the_preregistration(self) -> None:
        future = producer.load_preregistration()["futureGeneration"]["files"]
        self.assertEqual(
            {path.as_posix() for path in producer.FUTURE_CHAIN_RELATIVES},
            {
                future["producerFingerprint"],
                future["freeRehearsalResult"],
                future["futureOneUseAuthority"],
            },
        )


class CommandLineBoundaryTests(unittest.TestCase):
    def test_production_check_refuses_before_rehearsal_or_any_host_input(self) -> None:
        calls: list[str] = []
        with mock.patch.object(
            producer,
            "load_preregistration",
            side_effect=lambda: calls.append("preregistration") or {"bindings": []},
        ), mock.patch.object(
            producer,
            "load_import_closure_correction",
            side_effect=lambda _record: calls.append("correction") or {"addedBindings": []},
        ), mock.patch.object(
            producer,
            "verify_bound_inputs",
            side_effect=lambda _record, _correction: calls.append("bindings") or [],
        ), mock.patch.object(
            producer,
            "_verify_future_chain_before_any_effect",
            side_effect=producer.SuccessorProducerV3Error("authority absent"),
        ), mock.patch.object(
            producer,
            "run_bound_rehearsal",
            side_effect=AssertionError("production-check reached host inputs"),
        ):
            self.assertEqual(producer.main(["production-check"]), 2)
        self.assertEqual(calls, ["preregistration", "correction", "bindings"])

    def test_rehearsal_cli_writes_one_canonical_result_and_never_overwrites(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            cas = root / "cas"
            scratch = root / "scratch"
            result = root / "result.json"
            launcher = root / "launcher"
            cas.mkdir()
            scratch.mkdir()
            launcher.write_bytes(b"launcher")
            expected = producer.canonical_json(
                {
                    "schema": producer.REHEARSAL_SCHEMA,
                    "status": "PASS-NO-IMAGE-PRODUCED",
                }
            )
            arguments = [
                "rehearsal",
                "--cas",
                str(cas),
                "--launcher",
                str(launcher),
                "--scratch",
                str(scratch),
                "--gpgv",
                "/usr/bin/gpgv",
                "--zstd",
                "/usr/bin/zstd",
                "--result",
                str(result),
            ]
            with mock.patch.object(
                producer, "run_bound_rehearsal", return_value=expected
            ) as run:
                self.assertEqual(producer.main(arguments), 0)
            self.assertEqual(result.read_bytes(), expected)
            self.assertEqual(run.call_count, 1)

            with mock.patch.object(
                producer,
                "run_bound_rehearsal",
                side_effect=AssertionError("existing result must stop first"),
            ):
                self.assertEqual(producer.main(arguments), 2)

    def test_bound_rehearsal_uses_v2_lock_v4_builder_and_no_old_producer(self) -> None:
        source = Path(producer.__file__).read_text(encoding="utf-8")
        self.assertIn("native_shadow_rootfs_portable_boot_arm64_v2", source)
        self.assertIn("native_shadow_rootfs_builder_boot_arm64_v1", source)
        self.assertIn("builder_v4.validate_source_lock", source)
        self.assertIn("builder_v4.nested_runtime_tree", source)
        self.assertNotIn("native_shadow_successor_produce_phase_arm64_v2", source)
        self.assertNotIn("native_shadow_boot_image_produce_arm64_v1", source)
        self.assertNotIn("subprocess", source)

    def test_launcher_v2_bytes_are_checked_by_size_and_digest(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            launcher = Path(temporary) / "launcher"
            launcher.write_bytes(b"sealed-launcher")
            with mock.patch.object(
                producer, "LAUNCHER_SIZE_BYTES", 15
            ), mock.patch.object(
                producer,
                "LAUNCHER_SHA256",
                hashlib.sha256(b"sealed-launcher").hexdigest(),
            ):
                self.assertEqual(producer.read_launcher_v2(launcher), b"sealed-launcher")
            launcher.write_bytes(b"different")
            with mock.patch.object(producer, "LAUNCHER_SIZE_BYTES", len(b"different")):
                with self.assertRaisesRegex(
                    producer.SuccessorProducerV3Error, "digest"
                ):
                    producer.read_launcher_v2(launcher)

    def test_source_lock_v2_symlink_is_rejected_before_materialisation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            store = root / "cas"
            store.mkdir()
            source = root / producer.SOURCE_LOCK_RELATIVE
            source.parent.mkdir(parents=True)
            source.symlink_to(
                producer.REPOSITORY_ROOT / producer.SOURCE_LOCK_RELATIVE
            )
            with mock.patch.object(
                producer, "REPOSITORY_ROOT", root
            ), mock.patch.object(
                producer.staging_measure,
                "assert_replay_tool",
                side_effect=lambda path: path,
            ), mock.patch.object(
                producer.release_gate,
                "materialize_runtime_lock",
                side_effect=AssertionError("a symlink reached materialisation"),
            ):
                with self.assertRaisesRegex(
                    producer.SuccessorProducerV3Error,
                    "source-lock v2 is not a regular non-symlink file",
                ):
                    producer.prepare_real_inputs(
                        artifact_store=store,
                        gpgv=Path("/usr/bin/gpgv"),
                        zstd=Path("/usr/bin/zstd"),
                    )

    def test_bound_rehearsal_restores_tempdir_and_rejects_scratch_drift(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            scratch = root / "scratch"
            cas = root / "cas"
            scratch.mkdir()
            cas.mkdir()
            previous = producer.tempfile.tempdir

            def dirty(**_kwargs: object) -> bytes:
                (scratch / "escaped").write_bytes(b"effect")
                return producer.canonical_json({"status": "PASS-NO-IMAGE-PRODUCED"})

            with mock.patch.object(
                producer,
                "prepare_real_inputs",
                return_value=({"validated": True}, {"nested": {}}),
            ), mock.patch.object(
                producer, "read_launcher_v2", return_value=b"launcher"
            ), mock.patch.object(
                producer, "run_free_rehearsal", side_effect=dirty
            ):
                with self.assertRaisesRegex(
                    producer.SuccessorProducerV3Error, "scratch tree changed"
                ):
                    producer.run_bound_rehearsal(
                        artifact_store=cas,
                        launcher=Path("/not/read"),
                        scratch_root=scratch,
                        gpgv=Path("/usr/bin/gpgv"),
                        zstd=Path("/usr/bin/zstd"),
                    )
            self.assertIs(producer.tempfile.tempdir, previous)


if __name__ == "__main__":
    unittest.main()
