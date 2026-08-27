"""The MAC.3 runtime serving gap, measured against the tree rather than recalled.

The launcher binary is sealed and the guest boots.  Neither fact says the
launcher would reach serving inside that guest, because reaching serving means
finding things at fixed paths that nothing currently puts there.  This measures
which things, how much material each needs, and how much of that material is
already in hand -- before any of it is built.

Two kinds of claim live in the record and they are not tested the same way.
Claims about tracked files are re-derived here from those files, so drift fails.
Claims about the developer machine's local cache cannot be re-derived on a clean
runner, so what is tested is that they are labelled as local observations and
carry the method that produced them, never that they are reproducible here.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import re
import unittest

REPO = pathlib.Path(__file__).resolve().parent.parent
RECORD = (
    REPO
    / "native/containment/native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json"
)


def load() -> dict:
    return json.loads(RECORD.read_text(encoding="utf-8"))


def digest_of(relative: str) -> str:
    return hashlib.sha256((REPO / relative).read_bytes()).hexdigest()


class RecordShapeTests(unittest.TestCase):
    def test_the_record_exists(self) -> None:
        self.assertTrue(RECORD.is_file())

    def test_the_schema_is_the_measurement_schema(self) -> None:
        self.assertEqual(
            load()["schema"],
            "boole.native-shadow.mac3-runtime-serving-gap-measurement.arm64.v1",
        )

    def test_the_status_says_measured_and_not_closed(self) -> None:
        self.assertEqual(
            load()["status"], "MAC3-RUNTIME-SERVING-GAP-MEASURED-NOT-CLOSED"
        )

    def test_nothing_was_produced_dispatched_or_booted(self) -> None:
        record = load()
        for key in (
            "productionDispatched",
            "bootPerformed",
            "acquisitionPerformed",
            "servingClaim",
            "activationAllowed",
        ):
            with self.subTest(key=key):
                self.assertFalse(record[key])

    def test_the_record_carries_no_verdict(self) -> None:
        record = load()
        for forbidden in ("verdict", "passed", "result", "servingReached"):
            with self.subTest(key=forbidden):
                self.assertNotIn(forbidden, record)


class LauncherStartupChainTests(unittest.TestCase):
    """The chain is read out of main(), so a reordered main() fails here."""

    def setUp(self) -> None:
        self.record = load()
        self.source = (
            REPO / "crates/boole-native-shadow-launcher/src/bin/boole-native-shadow-launcher.rs"
        ).read_text(encoding="utf-8")

    def test_the_binary_digest_matches_the_file_it_names(self) -> None:
        chain = self.record["launcherStartupChain"]
        self.assertEqual(
            chain["source"]["sha256"], digest_of(chain["source"]["path"])
        )

    def test_every_named_stage_really_is_called_in_main(self) -> None:
        for stage in self.record["launcherStartupChain"]["stages"]:
            with self.subTest(stage=stage["call"]):
                self.assertIn(stage["call"], self.source)

    def _main_body(self) -> str:
        """Only main(), so the alphabetised import block cannot set the order."""
        start = self.source.index("fn main() -> Result<(), Box<dyn std::error::Error>>")
        return self.source[start:]

    def test_the_stages_are_recorded_in_the_order_main_calls_them(self) -> None:
        body = self._main_body()
        positions = [
            body.index(stage["call"])
            for stage in self.record["launcherStartupChain"]["stages"]
        ]
        self.assertEqual(positions, sorted(positions))

    def test_the_record_covers_every_startup_function_the_binary_imports(self) -> None:
        """A stage added to the binary without being recorded here fails."""
        imports = self.source.split("use boole_native_shadow_launcher::{")[1].split("};")[0]
        imported = set(re.findall(r"::([a-z][a-z0-9_]+),?\s*$", imports, re.MULTILINE))
        recorded = {
            stage["call"] for stage in self.record["launcherStartupChain"]["stages"]
        }
        self.assertEqual(imported - recorded, set())
        self.assertEqual(len(recorded), 9)

    def test_the_rootfs_check_is_not_the_last_thing_main_does(self) -> None:
        """Serving comes after it, which is why a failed check is never served."""
        stages = self.record["launcherStartupChain"]["stages"]
        calls = [stage["call"] for stage in stages]
        self.assertIn("verify_runtime_rootfs_replay", calls)
        self.assertEqual(calls.index("verify_runtime_rootfs_replay"), 6)
        self.assertLess(
            calls.index("verify_runtime_rootfs_replay"), len(calls) - 1
        )


class FixedGuestPathTests(unittest.TestCase):
    def setUp(self) -> None:
        self.record = load()
        self.source = (
            REPO / "crates/boole-native-shadow-launcher/src/bin/boole-native-shadow-launcher.rs"
        ).read_text(encoding="utf-8")

    def test_both_required_paths_are_recorded(self) -> None:
        paths = {row["guestPath"] for row in self.record["fixedGuestPathsRequired"]}
        self.assertEqual(
            paths,
            {
                "/var/lib/boole/native-shadow/runtime-rootfs",
                "/var/lib/boole/native-shadow/ROOTFS-CONTENT-MANIFEST.json",
            },
        )

    def test_each_recorded_path_is_a_literal_in_the_launcher_binary(self) -> None:
        for row in self.record["fixedGuestPathsRequired"]:
            with self.subTest(path=row["guestPath"]):
                self.assertIn(f'"{row["guestPath"]}"', self.source)

    def test_the_paths_are_compiled_in_rather_than_read_from_the_environment(
        self,
    ) -> None:
        """A configurable path would make the measurement meaningless."""
        self.assertNotIn("env::var", self.source)
        self.assertNotIn("std::env", self.source)

    def test_the_manifest_expectation_matches_what_the_launcher_compiles_against(
        self,
    ) -> None:
        authority = (
            REPO / "crates/boole-native-shadow-launcher/src/authority_arch.rs"
        ).read_text(encoding="utf-8")
        manifest = [
            row
            for row in self.record["fixedGuestPathsRequired"]
            if row["guestPath"].endswith("ROOTFS-CONTENT-MANIFEST.json")
        ][0]

        def arm64_value(constant: str) -> str:
            """The value guarded by the arm64 cfg, not the x86_64 one below it."""
            pattern = (
                r'#\[cfg\(all\(feature = "linux-arm64-authority", '
                r'any\(target_os = "linux", test\)\)\)\]\s*\n'
                r"pub\(crate\) const " + constant + r"[^=]*=\s*([^;]+);"
            )
            found = re.search(pattern, authority)
            self.assertIsNotNone(found, f"{constant} has no arm64 arm")
            return found.group(1).strip().strip('"')

        self.assertEqual(
            arm64_value("RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256"),
            manifest["expectedSha256"],
        )
        self.assertEqual(
            arm64_value("RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE"),
            f"{manifest['expectedSizeBytes']:_}",
        )
        self.assertEqual(
            arm64_value("RUNTIME_ROOTFS_CONTENT_MANIFEST_SCHEMA"),
            manifest["expectedSchema"],
        )

    def test_the_arm64_manifest_expectation_differs_from_the_x86_64_one(self) -> None:
        """Reading the wrong architecture's pin is the mistake this rules out."""
        authority = (
            REPO / "crates/boole-native-shadow-launcher/src/authority_arch.rs"
        ).read_text(encoding="utf-8")
        digests = re.findall(
            r'pub\(crate\) const RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256: &str =\s*"([0-9a-f]{64})"',
            authority,
        )
        self.assertEqual(len(digests), 2)
        self.assertNotEqual(digests[0], digests[1])

    def test_the_rootfs_must_be_a_read_only_mount(self) -> None:
        replay = (
            REPO / "crates/boole-native-shadow-launcher/src/runtime_rootfs_replay.rs"
        ).read_text(encoding="utf-8")
        row = [
            r
            for r in self.record["fixedGuestPathsRequired"]
            if r["guestPath"].endswith("runtime-rootfs")
        ][0]
        self.assertTrue(row["mustBeReadOnlyMount"])
        self.assertIn("require_read_only_mount", replay)


class TheGapItselfTests(unittest.TestCase):
    """The measured gap is that the image builder has never heard of these paths."""

    def setUp(self) -> None:
        self.record = load()
        self.gap = self.record["whatTheImageBuilderKnows"]

    def test_the_builder_digest_matches_the_file_it_names(self) -> None:
        self.assertEqual(self.gap["sha256"], digest_of(self.gap["path"]))

    def test_the_builder_really_mentions_none_of_the_required_paths(self) -> None:
        builder = (REPO / self.gap["path"]).read_text(encoding="utf-8")
        for row in self.record["fixedGuestPathsRequired"]:
            with self.subTest(path=row["guestPath"]):
                self.assertNotIn(row["guestPath"], builder)

    def test_the_recorded_mention_count_is_zero_and_true(self) -> None:
        builder = (REPO / self.gap["path"]).read_text(encoding="utf-8")
        self.assertEqual(self.gap["mentionsOfRequiredPaths"], 0)
        self.assertEqual(builder.count("/var/lib/boole/native-shadow"), 0)

    def test_the_consequence_is_stated_rather_than_left_to_inference(self) -> None:
        self.assertIn("fail", self.gap["consequence"].lower())


class MaterialInventoryTests(unittest.TestCase):
    """Counts that come from tracked locks are re-derived; local counts are not."""

    def setUp(self) -> None:
        self.record = load()
        self.inventory = self.record["materialInventory"]

    def _artifacts(self, relative: str) -> list:
        data = json.loads((REPO / relative).read_text(encoding="utf-8"))

        def find(node):
            if isinstance(node, dict):
                for key, value in node.items():
                    if key == "artifacts" and isinstance(value, list):
                        return value
                    found = find(value)
                    if found is not None:
                        return found
            elif isinstance(node, list):
                for value in node:
                    found = find(value)
                    if found is not None:
                        return found
            return None

        return find(data) or []

    def test_both_locks_are_recorded(self) -> None:
        roles = {row["role"] for row in self.inventory["locks"]}
        self.assertEqual(roles, {"boot-rootfs", "runtime-rootfs"})

    def test_each_lock_digest_matches_the_file_it_names(self) -> None:
        for row in self.inventory["locks"]:
            with self.subTest(role=row["role"]):
                self.assertEqual(row["sha256"], digest_of(row["path"]))

    def test_each_required_artifact_count_matches_the_lock(self) -> None:
        for row in self.inventory["locks"]:
            with self.subTest(role=row["role"]):
                self.assertEqual(
                    row["artifactsRequired"], len(self._artifacts(row["path"]))
                )

    def test_the_runtime_lock_is_the_smaller_of_the_two(self) -> None:
        by_role = {row["role"]: row for row in self.inventory["locks"]}
        self.assertLess(
            by_role["runtime-rootfs"]["artifactsRequired"],
            by_role["boot-rootfs"]["artifactsRequired"],
        )

    def test_local_presence_is_labelled_as_a_local_observation(self) -> None:
        """A clean runner has no such cache, so this must never read as a repo fact."""
        for row in self.inventory["locks"]:
            with self.subTest(role=row["role"]):
                self.assertTrue(row["localPresence"]["observedOnDeveloperMachine"])
                self.assertFalse(row["localPresence"]["reproducibleOnACleanRunner"])

    def test_local_presence_records_the_method_that_produced_it(self) -> None:
        for row in self.inventory["locks"]:
            with self.subTest(role=row["role"]):
                self.assertTrue(row["localPresence"]["method"].strip())

    def test_nothing_was_missing_and_that_is_stated_as_a_number(self) -> None:
        for row in self.inventory["locks"]:
            with self.subTest(role=row["role"]):
                presence = row["localPresence"]
                self.assertEqual(presence["absent"], 0)
                self.assertEqual(presence["present"], row["artifactsRequired"])

    def test_the_tests_here_do_not_read_the_local_cache(self) -> None:
        """This file must pass on a runner where the untracked cache is absent."""
        source = pathlib.Path(__file__).read_text(encoding="utf-8")
        cache = self.inventory["locks"][0]["localPresence"]["cacheDirectory"]
        self.assertNotIn(cache, source)
        # Built rather than written, so this line is not itself a match.
        self.assertNotIn(cache.split("/")[0] + "/", source)


class NotAnAcquisitionTests(unittest.TestCase):
    """The layer is derived from sealed inputs, and CI rebuilds it every PR."""

    def setUp(self) -> None:
        self.record = load()
        self.section = self.record["whyThisIsNotAnAcquisition"]

    def test_the_rebuilding_ci_job_is_named(self) -> None:
        workflow = (REPO / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn(self.section["ciJob"], workflow)

    def test_the_rebuilding_script_is_named_and_run_by_that_job(self) -> None:
        workflow = (REPO / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        self.assertIn(self.section["ciScript"], workflow)
        self.assertTrue((REPO / self.section["ciScript"]).is_file())

    def test_the_script_digest_matches_the_file_it_names(self) -> None:
        self.assertEqual(
            self.section["ciScriptSha256"], digest_of(self.section["ciScript"])
        )

    def test_the_expectation_the_rebuild_is_checked_against_is_named(self) -> None:
        self.assertEqual(
            self.section["expectationSha256"],
            digest_of(self.section["expectationPath"]),
        )

    def test_the_expected_layer_size_matches_the_sealed_expectation(self) -> None:
        expectation = json.loads(
            (REPO / self.section["expectationPath"]).read_text(encoding="utf-8")
        )
        self.assertEqual(
            self.section["derivedLayerSizeBytes"],
            expectation["expectedOutput"]["layerSizeBytes"],
        )

    def test_no_new_external_acquisition_is_claimed_to_be_needed(self) -> None:
        self.assertFalse(self.section["newExternalAcquisitionRequired"])


class CorrectionTests(unittest.TestCase):
    """An earlier reading in this session was wrong, and says so in the record."""

    def setUp(self) -> None:
        self.correction = load()["correctionOfAnEarlierReading"]

    def test_the_wrong_conclusion_is_stated_rather_than_quietly_dropped(self) -> None:
        self.assertTrue(self.correction["whatWasConcluded"].strip())
        self.assertTrue(self.correction["whyItWasWrong"].strip())

    def test_the_correction_names_what_would_have_happened(self) -> None:
        self.assertTrue(self.correction["whatItWouldHaveCost"].strip())

    def test_the_correction_is_not_dressed_up_as_a_finding(self) -> None:
        self.assertFalse(self.correction["wasAHardStop"])


class RemainingWorkTests(unittest.TestCase):
    def setUp(self) -> None:
        self.record = load()
        self.steps = self.record["remainingStepsToServing"]

    def test_the_steps_are_ordered_without_gaps(self) -> None:
        self.assertEqual(
            [step["order"] for step in self.steps],
            list(range(1, len(self.steps) + 1)),
        )

    def test_every_step_says_what_it_changes(self) -> None:
        for step in self.steps:
            with self.subTest(order=step["order"]):
                self.assertTrue(step["what"].strip())
                self.assertTrue(step["changes"].strip())

    def test_the_one_shot_steps_are_marked_as_one_shot(self) -> None:
        one_shot = [step for step in self.steps if step.get("oneShot")]
        self.assertGreaterEqual(len(one_shot), 2)

    def test_the_sealed_chain_records_are_named_by_path_and_still_exist(self) -> None:
        for step in self.steps:
            for path in step.get("touchesSealedRecords", []):
                with self.subTest(path=path):
                    self.assertTrue((REPO / path).is_file())

    def test_why_it_is_not_walked_now_is_recorded(self) -> None:
        self.assertTrue(self.record["whyNotWalkedInThisSession"].strip())


class WhatThisDoesNotEstablishTests(unittest.TestCase):
    def test_the_limits_are_enumerated(self) -> None:
        self.assertGreaterEqual(len(load()["notEstablishedByThisMeasurement"]), 3)

    def test_it_does_not_claim_the_launcher_would_serve(self) -> None:
        blob = json.dumps(load()["notEstablishedByThisMeasurement"]).lower()
        self.assertIn("serv", blob)

    def test_the_boundaries_are_present(self) -> None:
        self.assertGreaterEqual(len(load()["boundaries"]), 4)


class RegistrationTests(unittest.TestCase):
    def test_the_gate_runs_this_file(self) -> None:
        self.assertIn(
            "scripts/test_native_shadow_mac3_runtime_serving_gap_measurement_arm64_v1.py",
            (REPO / "scripts/self-test.sh").read_text(encoding="utf-8"),
        )

    def test_the_docs_smoke_pins_the_record(self) -> None:
        smoke = (REPO / "scripts/docs-smoke.sh").read_text(encoding="utf-8")
        self.assertIn(
            "native-shadow-mac3-runtime-serving-gap-measurement-arm64-v1.json", smoke
        )
        self.assertIn("MAC3-RUNTIME-SERVING-GAP-MEASURED-NOT-CLOSED", smoke)


if __name__ == "__main__":
    unittest.main()
