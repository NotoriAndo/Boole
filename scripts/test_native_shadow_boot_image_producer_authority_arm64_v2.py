"""Tests for the successor image producer authority (arm64 v2).

The v1 image builder authority froze the outputs, the ext4 determinism knobs,
the tool binaries and the prohibitions, and deliberately left two slots open:
`determinism.sourceDateEpoch` and the zstd host tool pin.  This successor fills
the first and states how the second is recorded, without editing the sealed v1
document.

The authority is frozen BEFORE anything is produced.  Nothing here builds or
boots; these tests assert what the producer will be held to.
"""

from __future__ import annotations

import json
import pathlib
import unittest

import scripts.native_shadow_boot_image_producer_authority_arm64_v2 as mod


REPO = pathlib.Path(__file__).resolve().parent.parent


def authority() -> dict:
    return mod.load_authority(REPO / mod.AUTHORITY_PATH)


class ConstantTests(unittest.TestCase):
    def test_format_is_declared_under_its_own_key(self) -> None:
        # The two earlier authority documents disagreed on this key name and a
        # fallback chain hid the difference.  This one states which key it uses.
        self.assertEqual(mod.FORMAT_KEY, "format")
        self.assertEqual(mod.FORMAT, "initrd-ext4-producer-authority-v2")
        self.assertEqual(authority()[mod.FORMAT_KEY], mod.FORMAT)

    def test_status_does_not_say_built_or_booted(self) -> None:
        self.assertEqual(
            mod.RESULT_STATUS, "IMAGE-PRODUCER-AUTHORITY-FROZEN-NOTHING-PRODUCED"
        )


class InheritedAuthorityTests(unittest.TestCase):
    def test_the_sealed_v1_authority_is_pinned_not_copied(self) -> None:
        # Restating mke2fs/debugfs digests here would create a second copy that
        # can drift from the sealed one, and a drift would be invisible.
        doc = authority()
        self.assertEqual(
            doc["builderAuthority"]["sha256"],
            "59a14469bbb9710a1f6c79202d3e804b2f79268966c12d4259cd99e59e8d6e1e",
        )
        text = json.dumps(doc)
        for digest in (
            "763be3ec03774647799b1186d30b4b524e6e73dd27be01cbe0be4b6043f62cb1",
            "2c0bf348d91f9b3bd6eec6666b9897b9f733c430e6baa8066bd70b645b2ca023",
        ):
            self.assertNotIn(digest, text, "tool digests must not be restated")

    def test_the_sealed_v1_authority_is_unchanged_on_disk(self) -> None:
        mod.check_inherited_authority(REPO, authority())

    def test_tool_pins_are_read_out_of_the_sealed_document(self) -> None:
        tools = mod.inherited_tool_binaries(REPO, authority())
        self.assertEqual(
            sorted(t["role"] for t in tools),
            ["ext4-image-inspector", "ext4-image-writer"],
        )
        for tool in tools:
            self.assertEqual(tool["package"], "e2fsprogs")
            self.assertEqual(len(tool["sha256"]), 64)


class OpenSlotTests(unittest.TestCase):
    def test_source_date_epoch_is_filled_and_agrees_with_canonical_mtime(self) -> None:
        doc = authority()
        inherited = json.loads(
            (REPO / doc["builderAuthority"]["path"]).read_text()
        )
        self.assertIsNone(inherited["determinism"]["sourceDateEpoch"])
        self.assertEqual(doc["determinism"]["sourceDateEpoch"], 0)
        self.assertEqual(doc["determinism"]["sourceDateEpoch"],
                         inherited["determinism"]["canonicalMtime"])

    def test_zstd_stays_pinned_at_build_time_not_guessed_here(self) -> None:
        doc = authority()
        self.assertEqual(doc["determinism"]["hostToolPinning"], "record-at-build-time")


class LauncherTests(unittest.TestCase):
    def test_the_launcher_is_rebuilt_and_checked_against_the_seal(self) -> None:
        # Receiving the ELF from another job would mean trusting a handoff.
        # Rebuilding and re-checking the digest trusts only the seal.
        doc = authority()
        self.assertEqual(doc["launcher"]["acquisition"], "rebuild-and-match-seal")
        self.assertEqual(
            doc["launcher"]["sha256"],
            "11b5d1cf1728aff271c589129292bcd8ad07a1d928652d2435b1c9010f73c434",
        )
        self.assertEqual(
            doc["launcher"]["guestLogicalPath"],
            "/usr/libexec/boole/boole-native-shadow-launcher",
        )

    def test_launcher_pin_agrees_with_the_sealed_build_result(self) -> None:
        doc = authority()
        sealed = json.loads((REPO / doc["launcher"]["resultPath"]).read_text())
        self.assertEqual(doc["launcher"]["sha256"], sealed["launcher"]["sha256"])
        self.assertEqual(
            doc["launcher"]["sizeBytes"], sealed["launcher"]["sizeBytes"]
        )
        self.assertEqual(
            doc["launcher"]["guestLogicalPath"], sealed["launcher"]["guestLogicalPath"]
        )


class IsolationTests(unittest.TestCase):
    def test_the_build_phase_has_no_network_namespace(self) -> None:
        # Policy text can fail open.  PrivateNetwork=yes means there is nothing
        # for a download to reach.
        properties = authority()["buildIsolation"]["systemdRunProperties"]
        self.assertIn("PrivateNetwork=yes", properties)
        self.assertIn("NoNewPrivileges=yes", properties)
        self.assertIn("RestrictAddressFamilies=AF_UNIX", properties)

    def test_acquire_and_build_are_separate_phases(self) -> None:
        phases = authority()["buildIsolation"]["phases"]
        self.assertEqual([p["name"] for p in phases], ["acquire", "produce"])
        self.assertIs(phases[0]["networkAllowed"], True)
        self.assertIs(phases[1]["networkAllowed"], False)


class OutputTests(unittest.TestCase):
    def test_outputs_match_the_sealed_roles(self) -> None:
        doc = authority()
        inherited = json.loads((REPO / doc["builderAuthority"]["path"]).read_text())
        self.assertEqual(doc["outputs"], inherited["roles"]["outputs"])

    def test_initrd_is_uncompressed(self) -> None:
        # An uncompressed cpio removes gzip's timestamp field as a source of
        # nondeterminism entirely, rather than working around it.
        doc = authority()
        initrd = [o for o in doc["outputs"] if o["name"] == "guest-initrd"][0]
        self.assertEqual(initrd["format"], "cpio-newc-uncompressed")

    def test_images_never_enter_git(self) -> None:
        retention = authority()["resultRetention"]
        self.assertIs(retention["commitImagesToGit"], False)
        self.assertIs(retention["uploadToRelease"], False)
        self.assertEqual(retention["keep"], "ci-artifact-and-sha256-manifest")


class AbortConditionTests(unittest.TestCase):
    def test_a_determinism_mismatch_is_a_hard_stop(self) -> None:
        doc = authority()
        self.assertEqual(
            doc["determinism"]["mismatchAction"],
            "report-the-difference-never-force-a-match",
        )
        self.assertIn("independent-builds-differ", mod.abort_ids(doc))

    def test_relaxing_a_determinism_knob_is_named_as_forbidden(self) -> None:
        forbidden = [
            a for a in doc_abort(authority()) if a["id"] == "independent-builds-differ"
        ][0]
        self.assertIs(forbidden["relaxKnobAllowed"], False)

    def test_every_operator_condition_has_an_abort_id(self) -> None:
        ids = mod.abort_ids(authority())
        for required in (
            "independent-builds-differ",
            "tool-binary-digest-mismatch",
            "network-attempted-during-produce",
            "maintainer-script-copied-into-tree",
            "output-missing-or-empty",
            "launcher-digest-mismatch",
        ):
            self.assertIn(required, ids)

    def test_the_maintainer_script_abort_is_about_the_tree_not_the_packages(
        self,
    ) -> None:
        """Maintainer scripts in the source packages are normal, not an abort.

        A read-only pass over the 191 frozen debs found 262 of them in the
        `control.tar` layers, which is what any Debian archive looks like.  The
        source lock says `never-execute-or-copy`, so the condition that must
        stop a build is one appearing in the assembled tree.  Wording it as
        "appeared in the consumed set" would abort every run that ever starts.
        """

        script = [
            a
            for a in doc_abort(authority())
            if a["id"] == "maintainer-script-copied-into-tree"
        ][0]
        self.assertIn("assembled tree", script["detects"])
        self.assertNotIn("consumed set", script["detects"])

    def test_two_packages_claiming_one_path_is_an_abort(self) -> None:
        """Nothing decides which of two colliding files wins.

        The same read-only pass found 0 collisions across 11,837 distinct
        non-directory paths, so union order is unobservable today.  That is a
        measurement of the current set, not a property of it -- if a collision
        ever appears, the tree becomes order-dependent and the two independent
        jobs can disagree for a reason no digest comparison would explain.
        """

        collision = [
            a for a in doc_abort(authority()) if a["id"] == "package-path-collision"
        ][0]
        self.assertIs(collision["relaxKnobAllowed"], False)
        self.assertIn("same logical path", collision["detects"])


def doc_abort(doc: dict) -> list:
    return doc["abortConditions"]


class BoundaryTests(unittest.TestCase):
    def test_freezing_an_authority_produces_nothing(self) -> None:
        boundaries = authority()["boundaries"]
        self.assertEqual([k for k, v in boundaries.items() if v], [])
        for flag in (
            "bootAuthority",
            "guestBootVerified",
            "guestImageBuilt",
            "initrdBuilt",
            "rootDiskBuilt",
            "launcherDeployedIntoGuest",
            "runtimeCompatibilityVerified",
            "toolByteProvenanceVerifiedInCi",
        ):
            self.assertIs(boundaries[flag], False, flag)

    def test_activation_stays_closed(self) -> None:
        doc = authority()
        self.assertIs(doc["activationAllowed"], False)
        self.assertIs(doc["bootableClaim"], False)


class GeneratorTests(unittest.TestCase):
    def test_the_self_pin_is_blanked_before_digesting(self) -> None:
        # The document names this tool and the tool names the document, so a
        # plain file digest could never equal the pin.  Changing only the pin
        # must leave the generator digest alone, or the pair is unsatisfiable.
        raw = (REPO / mod.GENERATOR_PATH).read_bytes()
        marker = b'AUTHORITY_SHA256 = "'
        start = raw.index(marker) + len(marker)
        other = raw[:start] + b"c" * 64 + raw[start + 64 :]
        self.assertNotEqual(raw, other)
        self.assertEqual(mod.generator_sha256(raw), mod.generator_sha256(other))

    def test_the_committed_generator_matches_its_pin(self) -> None:
        mod.check_generator(REPO, authority())

    def test_the_document_matches_its_own_pin(self) -> None:
        raw = (REPO / mod.AUTHORITY_PATH).read_bytes()
        self.assertEqual(mod.sha256_bytes(raw), mod.AUTHORITY_SHA256)


class RegenerationTests(unittest.TestCase):
    def test_regenerating_reproduces_the_committed_bytes(self) -> None:
        rebuilt = mod.canonical_bytes(mod.build_document(REPO))
        self.assertEqual(rebuilt, (REPO / mod.AUTHORITY_PATH).read_bytes())

    def test_verify_passes_against_this_repository(self) -> None:
        report = mod.verify(REPO)
        self.assertEqual(report["status"], mod.RESULT_STATUS)
        self.assertEqual(report["outputs"], 3)
        self.assertEqual(report["toolBinaries"], 2)


class InheritedDriftTests(unittest.TestCase):
    def test_a_lowered_source_date_epoch_is_caught(self) -> None:
        doc = authority()
        doc["determinism"]["sourceDateEpoch"] = 1
        with self.assertRaises(mod.ProducerAuthorityError):
            mod.check_inherited_authority(REPO, doc)

    def test_a_softened_mismatch_action_is_caught(self) -> None:
        doc = authority()
        doc["determinism"]["mismatchAction"] = "retry-until-they-agree"
        with self.assertRaises(mod.ProducerAuthorityError):
            mod.check_inherited_authority(REPO, doc)

    def test_outputs_that_drift_from_the_sealed_roles_are_caught(self) -> None:
        doc = authority()
        doc["outputs"] = doc["outputs"][:2]
        with self.assertRaises(mod.ProducerAuthorityError):
            mod.check_inherited_authority(REPO, doc)


class LoadTests(unittest.TestCase):
    def test_non_canonical_bytes_are_refused(self) -> None:
        import tempfile

        doc = authority()
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp).resolve() / "authority.json"
            path.write_text(json.dumps(doc))  # compact, not canonical
            with self.assertRaises(mod.ProducerAuthorityError):
                mod.load_authority(path)

    def test_a_changed_inherited_digest_is_refused(self) -> None:
        doc = authority()
        doc["builderAuthority"]["sha256"] = "b" * 64
        with self.assertRaises(mod.ProducerAuthorityError):
            mod.check_inherited_authority(REPO, doc)


if __name__ == "__main__":
    unittest.main()
