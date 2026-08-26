"""Tests for the successor boot artifact build plan (arm64 v2).

The audit-only v1 preflight refuses a plan whose three authority slots carry a
digest: "belongs to an audit-only scaffold and must stay null; use a successor
plan/schema/tool".  This module is that successor, so its rules are the mirror
image -- here a null slot is the error -- plus the asymmetry a uniform
implementation would get wrong.
"""

from __future__ import annotations

import json
import pathlib
import tempfile
import unittest

import scripts.native_shadow_boot_artifact_plan_arm64_v2 as mod


REPO = pathlib.Path(__file__).resolve().parent.parent


class ConstantTests(unittest.TestCase):
    def test_schema_is_a_successor_not_a_reuse(self) -> None:
        # Reusing the v1 schema would let the audit-only preflight load a
        # resolved plan and reject it -- the exact failure this slice avoids.
        self.assertEqual(
            mod.PLAN_SCHEMA, "boole.native-shadow.boot-artifact-build-plan.arm64.v2"
        )
        self.assertNotEqual(mod.PLAN_SCHEMA, mod.SCAFFOLD_SCHEMA)

    def test_slot_formats_match_the_scaffold_exactly(self) -> None:
        self.assertEqual(
            mod.SLOT_FORMATS,
            {
                "imageBuilderToolchain": "initrd-ext4-builder-authority-v1",
                "kernel": "linux-arm64-image",
                "systemdGuestClosure": "systemd-rootfs-closure-authority-v1",
            },
        )

    def test_document_slots_and_artifact_slots_are_distinguished(self) -> None:
        # Two slots pin an authority DOCUMENT that declares its own format.
        # One pins raw IMAGE bytes and has no document to ask.
        self.assertEqual(
            mod.DOCUMENT_SLOTS, ("imageBuilderToolchain", "systemdGuestClosure")
        )
        self.assertEqual(mod.ARTIFACT_SLOTS, ("kernel",))
        self.assertEqual(
            set(mod.DOCUMENT_SLOTS) | set(mod.ARTIFACT_SLOTS), set(mod.SLOT_FORMATS)
        )

    def test_status_does_not_say_boot(self) -> None:
        self.assertEqual(
            mod.RESULT_STATUS, "BOOT-INPUT-AUTHORITIES-RESOLVED-NOT-BOOT-AUTHORITY"
        )


class ResolvedPinTests(unittest.TestCase):
    """Mirror of the v1 rule: here null is the error."""

    def _pin(self, **over: object) -> dict[str, object]:
        pin = {"format": "linux-arm64-image", "sha256": "a" * 64, "sizeBytes": 1}
        pin.update(over)
        return pin

    def test_a_fully_resolved_pin_is_accepted(self) -> None:
        self.assertEqual(mod.resolved_pin(self._pin(), "kernel"), self._pin())

    def test_null_digest_is_refused(self) -> None:
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.resolved_pin(self._pin(sha256=None), "kernel")

    def test_null_size_is_refused(self) -> None:
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.resolved_pin(self._pin(sizeBytes=None), "kernel")

    def test_zero_size_is_refused(self) -> None:
        # An empty artifact hashes fine; size is the check that catches it.
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.resolved_pin(self._pin(sizeBytes=0), "kernel")

    def test_uppercase_digest_is_refused(self) -> None:
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.resolved_pin(self._pin(sha256="A" * 64), "kernel")

    def test_short_digest_is_refused(self) -> None:
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.resolved_pin(self._pin(sha256="a" * 63), "kernel")

    def test_boolean_size_is_refused(self) -> None:
        # bool is an int in Python; a size of True must not pass as 1.
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.resolved_pin(self._pin(sizeBytes=True), "kernel")

    def test_wrong_format_for_slot_is_refused(self) -> None:
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.resolved_pin(
                self._pin(format="systemd-rootfs-closure-authority-v1"), "kernel"
            )

    def test_extra_key_in_a_pin_is_refused(self) -> None:
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.resolved_pin(self._pin(note="trust me"), "kernel")


class DocumentSlotTests(unittest.TestCase):
    def test_the_two_documents_use_different_key_names(self) -> None:
        # This is the trap.  A reader that tried "format" and fell back to
        # "closureFormat" would accept either document in either slot, so the
        # key is pinned per slot.  Both real files are read here rather than
        # trusted from memory.
        self.assertEqual(
            mod.SLOT_FORMAT_KEYS,
            {"imageBuilderToolchain": "format", "systemdGuestClosure": "closureFormat"},
        )
        for slot, key in mod.SLOT_FORMAT_KEYS.items():
            document = json.loads((REPO / mod.SLOT_DOCUMENTS[slot]).read_text())
            self.assertEqual(document[key], mod.SLOT_FORMATS[slot], slot)

    def test_a_document_using_the_other_slots_key_is_refused(self) -> None:
        # Right value, wrong field name -- must not be waved through.
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.check_document_format(
                "systemdGuestClosure",
                {"format": "systemd-rootfs-closure-authority-v1"},
            )

    def test_document_declaring_a_different_format_is_refused(self) -> None:
        # A document whose bytes hash correctly but which declares a different
        # format is the wrong object in a right-looking slot.
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.check_document_format(
                "systemdGuestClosure",
                {"closureFormat": "initrd-ext4-builder-authority-v1"},
            )

    def test_document_missing_a_format_field_is_refused(self) -> None:
        # This is exactly the kernel-result shape.  It must not be accepted into
        # a document slot just because its digest was pinned correctly.
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.check_document_format("systemdGuestClosure", {"schema": "something"})

    def test_matching_format_is_accepted(self) -> None:
        mod.check_document_format(
            "systemdGuestClosure",
            {"closureFormat": "systemd-rootfs-closure-authority-v1"},
        )
        mod.check_document_format(
            "imageBuilderToolchain",
            {"format": "initrd-ext4-builder-authority-v1"},
        )


class ScaffoldUntouchedTests(unittest.TestCase):
    def test_the_v1_scaffold_still_carries_null_slots(self) -> None:
        # The successor exists so the scaffold never has to be edited.  If the
        # scaffold ever gains a digest, the v1 preflight starts failing and this
        # test says why.
        scaffold = json.loads((REPO / mod.SCAFFOLD_PATH).read_text())
        self.assertEqual(scaffold["schema"], mod.SCAFFOLD_SCHEMA)
        for slot in mod.SLOT_FORMATS:
            pin = scaffold["inputs"][slot]
            self.assertIsNone(pin["sha256"], slot)
            self.assertIsNone(pin["sizeBytes"], slot)


class VerifyAgainstRepoTests(unittest.TestCase):
    def test_every_slot_agrees_with_its_tracked_document(self) -> None:
        report = mod.verify_slots(REPO, mod.load_plan(REPO / mod.PLAN_PATH))
        self.assertEqual(sorted(report), sorted(mod.SLOT_FORMATS))
        for slot, row in report.items():
            self.assertTrue(row["digestMatches"], slot)
            self.assertTrue(row["sizeMatches"], slot)
            self.assertTrue(row["reproducibleInCi"], slot)

    def test_a_tampered_pin_is_caught(self) -> None:
        plan = mod.load_plan(REPO / mod.PLAN_PATH)
        plan["inputs"]["systemdGuestClosure"]["sha256"] = "b" * 64
        with self.assertRaises(mod.BootArtifactPlanError):
            mod.verify_slots(REPO, plan)

    def test_kernel_pin_is_checked_against_the_sealed_result(self) -> None:
        # The kernel image bytes are not in the repo, but the sealed C1 result
        # is, so the pin's agreement with it is fully CI-reproducible.  What CI
        # cannot confirm is that those bytes exist anywhere -- a separate fact,
        # recorded separately.
        sealed = json.loads((REPO / mod.KERNEL_RESULT_PATH).read_text())
        plan = mod.load_plan(REPO / mod.PLAN_PATH)
        self.assertEqual(
            plan["inputs"]["kernel"]["sha256"], sealed["kernel"]["sha256"]
        )
        self.assertEqual(
            plan["inputs"]["kernel"]["sizeBytes"], sealed["kernel"]["sizeBytes"]
        )

    def test_plan_rejects_an_unknown_slot(self) -> None:
        plan = mod.load_plan(REPO / mod.PLAN_PATH)
        plan["inputs"]["extraAuthority"] = {
            "format": "linux-arm64-image",
            "sha256": "a" * 64,
            "sizeBytes": 1,
        }
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp).resolve() / "plan.json"
            path.write_bytes(mod.canonical_bytes(plan))
            with self.assertRaises(mod.BootArtifactPlanError):
                mod.load_plan(path)


class BoundaryTests(unittest.TestCase):
    def test_resolving_a_plan_does_not_build_or_boot_anything(self) -> None:
        for flag in (
            "bootAuthority",
            "guestBootVerified",
            "guestImageBuilt",
            "initrdBuilt",
            "rootDiskBuilt",
            "launcherDeployedIntoGuest",
            "runtimeCompatibilityVerified",
        ):
            self.assertIs(mod.BOUNDARIES[flag], False, flag)

    def test_only_the_input_resolution_flag_is_true(self) -> None:
        self.assertIs(mod.BOUNDARIES["bootInputAuthoritiesResolved"], True)
        true_flags = [k for k, v in mod.BOUNDARIES.items() if v]
        self.assertEqual(true_flags, ["bootInputAuthoritiesResolved"])

    def test_activation_stays_closed(self) -> None:
        self.assertIs(mod.ACTIVATION_ALLOWED, False)
        plan = mod.load_plan(REPO / mod.PLAN_PATH)
        self.assertIs(plan["activationAllowed"], False)

    def test_plan_does_not_claim_bootability(self) -> None:
        plan = mod.load_plan(REPO / mod.PLAN_PATH)
        self.assertIs(plan["bootableClaim"], False)


if __name__ == "__main__":
    unittest.main()
