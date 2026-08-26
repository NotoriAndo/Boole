#!/usr/bin/env python3
"""Acceptance tests for the Linux/arm64 launcher build authority.

These run anywhere.  They never compile the launcher -- the real double build
belongs on the arm64 runner -- so every test here exercises the contract that
decides whether a build result may be believed, not the build itself.
"""

from __future__ import annotations

import copy
import json
import pathlib
import sys
import tempfile
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts import native_shadow_launcher_build_arm64_v1 as launcher


def _load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


class AuthorityAcceptanceTests(unittest.TestCase):
    """A mutated authority must be refused rather than quietly accepted."""

    def setUp(self) -> None:
        self.authority = _load(launcher.AUTHORITY_PATH)

    def mutate(self) -> dict:
        return copy.deepcopy(self.authority)

    def assertRefused(self, document: dict, needle: str) -> None:
        with self.assertRaises(launcher.LauncherBuildError) as caught:
            launcher.validate_authority(document)
        self.assertIn(needle, str(caught.exception))

    def test_the_frozen_authority_is_accepted(self) -> None:
        self.assertEqual(
            launcher.validate_authority(self.mutate())["schema"],
            launcher.AUTHORITY_SCHEMA,
        )

    def test_an_extra_key_is_refused(self) -> None:
        document = self.mutate()
        document["extra"] = True
        self.assertRefused(document, "authority keys differ")

    def test_a_dropped_key_is_refused(self) -> None:
        document = self.mutate()
        document.pop("determinism")
        self.assertRefused(document, "authority keys differ")

    def test_activation_cannot_be_claimed(self) -> None:
        document = self.mutate()
        document["activationAllowed"] = True
        self.assertRefused(document, "must not claim activation or boot")

    def test_a_boot_claim_is_refused(self) -> None:
        document = self.mutate()
        document["bootableClaim"] = True
        self.assertRefused(document, "must not claim activation or boot")

    def test_every_boundary_must_stay_false(self) -> None:
        for name in sorted(launcher.BOUNDARY_KEYS):
            document = self.mutate()
            document["boundaries"][name] = True
            self.assertRefused(document, f"boundary {name} must stay false")

    def test_a_missing_boundary_is_refused(self) -> None:
        document = self.mutate()
        document["boundaries"].pop("bootAuthority")
        self.assertRefused(document, "boundaries keys differ")

    def test_toolchain_byte_provenance_cannot_be_declared_closed(self) -> None:
        document = self.mutate()
        document["toolchain"]["byteProvenanceClosed"] = True
        self.assertRefused(document, "toolchain differs")

    def test_a_different_toolchain_channel_is_refused(self) -> None:
        document = self.mutate()
        document["toolchain"]["channel"] = "nightly"
        self.assertRefused(document, "toolchain differs")

    def test_a_foreign_target_triple_is_refused(self) -> None:
        document = self.mutate()
        document["platform"]["rustTarget"] = "x86_64-unknown-linux-gnu"
        self.assertRefused(document, "platform differs")

    def test_dropping_locked_from_the_build_command_is_refused(self) -> None:
        document = self.mutate()
        document["build"]["command"] = [
            part for part in document["build"]["command"] if part != "--locked"
        ]
        self.assertRefused(document, "build recipe differs")

    def test_dropping_offline_from_the_build_command_is_refused(self) -> None:
        document = self.mutate()
        document["build"]["command"] = [
            part for part in document["build"]["command"] if part != "--offline"
        ]
        self.assertRefused(document, "build recipe differs")

    def test_switching_the_profile_is_refused(self) -> None:
        document = self.mutate()
        document["build"]["profile"] = "debug"
        self.assertRefused(document, "build recipe differs")

    def test_relaxing_overflow_checks_is_refused(self) -> None:
        document = self.mutate()
        document["build"]["profileFlags"]["overflowChecks"] = False
        self.assertRefused(document, "build recipe differs")

    def test_a_single_build_is_refused(self) -> None:
        document = self.mutate()
        document["determinism"]["independentBuildCount"] = 1
        self.assertRefused(document, "determinism contract differs")

    def test_allowing_a_non_identical_artifact_is_refused(self) -> None:
        document = self.mutate()
        document["determinism"]["artifactMustBeByteIdentical"] = False
        self.assertRefused(document, "determinism contract differs")

    def test_permitting_timestamp_suppression_is_refused(self) -> None:
        document = self.mutate()
        document["determinism"]["forbidTimestampSuppression"] = False
        self.assertRefused(document, "determinism contract differs")

    def test_source_files_must_be_sorted(self) -> None:
        document = self.mutate()
        document["sourceFiles"].reverse()
        self.assertRefused(document, "not sorted by path")

    def test_a_duplicated_source_path_is_refused(self) -> None:
        document = self.mutate()
        document["sourceFiles"].append(copy.deepcopy(document["sourceFiles"][0]))
        self.assertRefused(document, "not sorted by path")

    def test_an_absolute_source_path_is_refused(self) -> None:
        document = self.mutate()
        document["sourceFiles"][0]["path"] = "/etc/passwd"
        self.assertRefused(document, "not repository-relative")

    def test_a_traversing_source_path_is_refused(self) -> None:
        document = self.mutate()
        document["sourceFiles"][0]["path"] = "crates/../../outside.rs"
        self.assertRefused(document, "not repository-relative")

    def test_a_short_source_digest_is_refused(self) -> None:
        document = self.mutate()
        document["sourceFiles"][0]["sha256"] = "abc"
        self.assertRefused(document, "not a sha256 digest")

    def test_an_uppercase_source_digest_is_refused(self) -> None:
        document = self.mutate()
        document["sourceFiles"][0]["sha256"] = document["sourceFiles"][0]["sha256"].upper()
        self.assertRefused(document, "not a sha256 digest")

    def test_an_empty_source_set_is_refused(self) -> None:
        document = self.mutate()
        document["sourceFiles"] = []
        self.assertRefused(document, "sourceFiles is empty")


class SourceDriftTests(unittest.TestCase):
    """The pinned sources must still be the sources on disk."""

    def setUp(self) -> None:
        self.authority = _load(launcher.AUTHORITY_PATH)

    def test_the_working_tree_matches_every_pinned_source(self) -> None:
        self.assertEqual(launcher.verify_sources(self.authority), [])

    def test_a_tampered_source_is_reported_as_drift(self) -> None:
        authority = copy.deepcopy(self.authority)
        authority["sourceFiles"][0]["sha256"] = "0" * 64
        self.assertEqual(
            launcher.verify_sources(authority), [authority["sourceFiles"][0]["path"]]
        )

    def test_a_resized_source_is_reported_as_drift(self) -> None:
        authority = copy.deepcopy(self.authority)
        authority["sourceFiles"][0]["sizeBytes"] += 1
        self.assertEqual(
            launcher.verify_sources(authority), [authority["sourceFiles"][0]["path"]]
        )

    def test_a_vanished_source_is_reported_as_drift(self) -> None:
        authority = copy.deepcopy(self.authority)
        authority["sourceFiles"][0]["path"] = "crates/does-not-exist.rs"
        self.assertEqual(launcher.verify_sources(authority), ["crates/does-not-exist.rs"])

    def test_the_launcher_binary_entrypoint_is_pinned(self) -> None:
        pinned = {row["path"] for row in self.authority["sourceFiles"]}
        self.assertIn(
            "crates/boole-native-shadow-launcher/src/bin/boole-native-shadow-launcher.rs",
            pinned,
        )
        self.assertIn("Cargo.lock", pinned)
        self.assertIn("rust-toolchain.toml", pinned)

    def test_ignored_local_debris_is_never_pinned(self) -> None:
        # `.DS_Store` sits untracked inside the launcher crate; a directory walk
        # would have swept it in and made the pin host-dependent.
        for row in self.authority["sourceFiles"]:
            self.assertNotIn(".DS_Store", row["path"])


class DoubleBuildTests(unittest.TestCase):
    """Two builds decide the verdict; a mismatch is reported, never smoothed."""

    def setUp(self) -> None:
        self.authority = _load(launcher.AUTHORITY_PATH)

    def test_identical_builds_are_accepted(self) -> None:
        calls: list[pathlib.Path] = []

        def builder(authority, *, workspace, repo_root):
            calls.append(workspace)
            return b"identical-launcher-elf"

        built = launcher.build_twice(self.authority, builder=builder)
        self.assertEqual(built["buildCount"], 2)
        self.assertEqual(built["sizeBytes"], len(b"identical-launcher-elf"))
        self.assertEqual(built["sha256"], launcher.sha256_bytes(b"identical-launcher-elf"))
        # Two builds, two distinct trees -- not one tree reused.
        self.assertEqual(len(calls), 2)
        self.assertNotEqual(calls[0], calls[1])

    def test_a_one_byte_difference_is_refused(self) -> None:
        outputs = [b"launcher-elf-a", b"launcher-elf-b"]

        def builder(authority, *, workspace, repo_root):
            return outputs.pop(0)

        with self.assertRaises(launcher.LauncherBuildError) as caught:
            launcher.build_twice(self.authority, builder=builder)
        self.assertIn("not byte-identical", str(caught.exception))

    def test_a_length_difference_is_refused(self) -> None:
        outputs = [b"launcher-elf", b"launcher-elf-longer"]

        def builder(authority, *, workspace, repo_root):
            return outputs.pop(0)

        with self.assertRaises(launcher.LauncherBuildError):
            launcher.build_twice(self.authority, builder=builder)

    def test_the_build_environment_remaps_the_source_root(self) -> None:
        root = pathlib.Path("/tmp/boole-launcher-build-0-abcdef")
        environment = launcher._build_environment(self.authority, root)
        flags = environment["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
        self.assertIn("--remap-path-prefix", flags)
        self.assertIn(f"{root}=/boole/launcher-build", flags)
        self.assertEqual(environment["CARGO_INCREMENTAL"], "0")
        self.assertEqual(environment["TZ"], "UTC")
        # A declared-but-null value is an explicit refusal to set it, not a
        # licence to invent one: nothing here suppresses a build timestamp.
        self.assertNotIn("SOURCE_DATE_EPOCH", environment)

    def test_two_trees_remap_to_the_same_logical_root(self) -> None:
        first = launcher._build_environment(self.authority, pathlib.Path("/tmp/build-one"))
        second = launcher._build_environment(self.authority, pathlib.Path("/tmp/build-two-x"))
        self.assertNotEqual(
            first["CARGO_ENCODED_RUSTFLAGS"], second["CARGO_ENCODED_RUSTFLAGS"]
        )
        for environment in (first, second):
            self.assertTrue(
                environment["CARGO_ENCODED_RUSTFLAGS"].endswith("=/boole/launcher-build")
            )


class ResultShapeTests(unittest.TestCase):
    """A sealed result may not outrun what a double build actually shows."""

    def setUp(self) -> None:
        self.authority = _load(launcher.AUTHORITY_PATH)
        self.built = {"buildCount": 2, "sha256": "a" * 64, "sizeBytes": 4096}
        self.identity = {
            "cargo": "cargo 1.95.0",
            "host": "aarch64-unknown-linux-gnu",
            "rustc": "rustc 1.95.0",
        }

    def test_the_result_carries_the_authority_and_the_verdict(self) -> None:
        result = launcher.build_result(self.authority, self.built, self.identity)
        self.assertEqual(result["schema"], launcher.RESULT_SCHEMA)
        self.assertEqual(result["release"], launcher.RELEASE)
        self.assertEqual(result["status"], launcher.RESULT_STATUS)
        self.assertEqual(result["authoritySha256"], launcher.AUTHORITY_SHA256)
        self.assertEqual(result["independentBuildCount"], 2)
        self.assertEqual(result["launcher"]["sha256"], "a" * 64)
        self.assertEqual(
            result["launcher"]["guestLogicalPath"],
            "/usr/libexec/boole/boole-native-shadow-launcher",
        )

    def test_the_result_claims_no_deployment_image_or_boot_authority(self) -> None:
        result = launcher.build_result(self.authority, self.built, self.identity)
        self.assertEqual(sorted(result["boundaries"]), sorted(launcher.BOUNDARY_KEYS))
        for name, value in result["boundaries"].items():
            self.assertIs(value, False, f"boundary {name} must stay false")
        self.assertIs(result["activationAllowed"], False)
        self.assertIs(result["bootableClaim"], False)
        # A reproducible ELF on a runner is not an ELF inside a guest image, and
        # it is certainly not a booted guest.
        self.assertIs(result["boundaries"]["launcherDeployedIntoGuest"], False)
        self.assertIs(result["boundaries"]["guestImageBuilt"], False)
        self.assertIs(result["boundaries"]["bootAuthority"], False)

    def test_the_result_records_the_observed_toolchain_not_a_provenance_claim(self) -> None:
        result = launcher.build_result(self.authority, self.built, self.identity)
        self.assertEqual(result["observedToolchain"], self.identity)
        self.assertIs(result["boundaries"]["toolchainByteProvenanceClosed"], False)
        self.assertEqual(
            self.authority["toolchain"]["identityProbeScope"],
            "version-and-host-only;not-installed-byte-provenance",
        )

    def test_the_result_is_canonical_json(self) -> None:
        result = launcher.build_result(self.authority, self.built, self.identity)
        raw = launcher.canonical_json(result)
        self.assertEqual(launcher.canonical_json(json.loads(raw.decode("utf-8"))), raw)


class HostGuardTests(unittest.TestCase):
    """The double build belongs on arm64 Linux and nowhere else."""

    def test_a_foreign_host_is_refused(self) -> None:
        if sys.platform == "linux" and __import__("os").uname().machine in {
            "aarch64",
            "arm64",
        }:
            self.skipTest("this host is the supported build host")
        with self.assertRaises(launcher.LauncherBuildError) as caught:
            launcher._require_arm64_linux()
        self.assertIn("Linux aarch64 only", str(caught.exception))


class LinkerDeclarationTests(unittest.TestCase):
    """The linker is an input that decides the bytes, so it is named, not omitted.

    Its bytes are not frozen -- it is whatever `cc` the runner image ships -- and
    that gap is stated rather than papered over.  Claiming a closed provenance
    here would be the dishonest option; leaving the input unmentioned would be
    the quieter one.
    """

    def setUp(self) -> None:
        self.authority = _load(launcher.AUTHORITY_PATH)

    def test_the_linker_input_is_declared(self) -> None:
        linker = self.authority["build"]["linker"]
        self.assertEqual(sorted(linker), ["byteProvenanceClosed", "selection"])
        self.assertIsInstance(linker["selection"], str)
        self.assertTrue(linker["selection"])

    def test_the_linker_never_claims_frozen_bytes(self) -> None:
        self.assertIs(self.authority["build"]["linker"]["byteProvenanceClosed"], False)

    def test_a_linker_claiming_closed_provenance_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        document["build"]["linker"]["byteProvenanceClosed"] = True
        with self.assertRaises(launcher.LauncherBuildError) as caught:
            launcher.validate_authority(document)
        self.assertIn("build recipe differs", str(caught.exception))

    def test_dropping_the_linker_is_refused(self) -> None:
        document = copy.deepcopy(self.authority)
        del document["build"]["linker"]
        with self.assertRaises(launcher.LauncherBuildError) as caught:
            launcher.validate_authority(document)
        self.assertIn("build recipe differs", str(caught.exception))


class GeneratorPinTests(unittest.TestCase):
    """The authority names the tool that wrote it, and that pin has to stay true.

    The tool carries the authority's digest as a literal, so the tool's own
    digest is taken with that literal blanked out.  That breaks the circle: the
    document can name the tool without the tool's pin changing the answer.
    """

    def setUp(self) -> None:
        self.authority = _load(launcher.AUTHORITY_PATH)

    def test_the_authority_names_the_tool_that_generated_it(self) -> None:
        generator = self.authority["generator"]
        self.assertEqual(sorted(generator), ["path", "sha256"])
        self.assertEqual(
            generator["path"], "scripts/native_shadow_launcher_build_arm64_v1.py"
        )
        self.assertEqual(launcher.TOOL_PATH, ROOT / generator["path"])

    def test_the_pinned_generator_digest_tracks_the_tool_on_disk(self) -> None:
        self.assertEqual(
            self.authority["generator"]["sha256"],
            launcher.launcher_build_authority_sha256(launcher.TOOL_PATH.read_bytes()),
            "editing the build tool must invalidate the pin, not slide past it",
        )

    def test_the_self_referential_pin_is_blanked_before_hashing(self) -> None:
        raw = launcher.TOOL_PATH.read_bytes()
        self.assertIn(launcher.AUTHORITY_SHA256.encode(), raw)
        rotated = raw.replace(launcher.AUTHORITY_SHA256.encode(), b"c" * 64)
        self.assertEqual(
            launcher.launcher_build_authority_sha256(raw),
            launcher.launcher_build_authority_sha256(rotated),
            "the document digest must not feed back into the tool digest",
        )

    def test_a_missing_or_misshapen_generator_is_refused(self) -> None:
        without = copy.deepcopy(self.authority)
        del without["generator"]
        with self.assertRaises(launcher.LauncherBuildError) as caught:
            launcher.validate_authority(without)
        self.assertIn("authority keys differ", str(caught.exception))

        for broken in ({"path": "scripts/x.py"}, {"path": 1, "sha256": "d" * 64},
                       {"path": "scripts/x.py", "sha256": "not-a-digest"}):
            document = copy.deepcopy(self.authority)
            document["generator"] = broken
            with self.assertRaises(launcher.LauncherBuildError) as caught:
                launcher.validate_authority(document)
            self.assertIn("generator", str(caught.exception))


class SealAndReproveTests(unittest.TestCase):
    """The first real build seals a digest; every later build re-proves it.

    The double build cannot run on the developer Mac, so the launcher digest is
    discovered on the arm64 runner.  That makes the seal a claim the runner must
    keep re-earning: a later build that produces different bytes has to surface
    the difference, never quietly rewrite the record to agree with itself.
    """

    def setUp(self) -> None:
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        # resolve(): the write-once primitive refuses a parent reached through a
        # symlink, and the macOS temp root is one.
        self.path = pathlib.Path(self.directory.name).resolve() / "result.json"
        self.result = {
            "activationAllowed": False,
            "bootableClaim": False,
            "launcher": {"sha256": "a" * 64, "sizeBytes": 4096},
            "release": launcher.RELEASE,
            "status": launcher.RESULT_STATUS,
        }

    def test_the_first_build_seals_the_result(self) -> None:
        self.assertEqual(
            launcher.seal_or_reprove(self.result, result_path=self.path), "sealed"
        )
        self.assertEqual(self.path.read_bytes(), launcher.canonical_json(self.result))

    def test_an_identical_rebuild_reproves_without_rewriting(self) -> None:
        launcher.seal_or_reprove(self.result, result_path=self.path)
        before = self.path.stat()
        self.assertEqual(
            launcher.seal_or_reprove(self.result, result_path=self.path), "re-proved"
        )
        after = self.path.stat()
        self.assertEqual(after.st_mtime_ns, before.st_mtime_ns)
        self.assertEqual(after.st_ino, before.st_ino)

    def test_a_divergent_rebuild_is_reported_never_overwritten(self) -> None:
        launcher.seal_or_reprove(self.result, result_path=self.path)
        sealed = self.path.read_bytes()
        divergent = copy.deepcopy(self.result)
        divergent["launcher"]["sha256"] = "b" * 64
        with self.assertRaises(launcher.LauncherBuildError) as caught:
            launcher.seal_or_reprove(divergent, result_path=self.path)
        self.assertIn("never overwrite the seal", str(caught.exception))
        self.assertEqual(self.path.read_bytes(), sealed)


class SealedDocumentTests(unittest.TestCase):
    """The authority is wired into the gates that would catch its drift."""

    def test_the_authority_is_registered_in_both_gates(self) -> None:
        self_test = (ROOT / "scripts" / "self-test.sh").read_text(encoding="utf-8")
        needle = "scripts/test_native_shadow_launcher_build_arm64_v1.py"
        self.assertTrue(needle in self_test, f"{needle} is not run by scripts/self-test.sh")
        smoke = (ROOT / "scripts" / "docs-smoke.sh").read_text(encoding="utf-8")
        document = (
            "native/containment/native-shadow-launcher-build-authority-arm64-v1.json"
        )
        self.assertTrue(document in smoke, f"{document} is not pinned by docs-smoke")

    def test_the_authority_digest_pin_matches_the_file(self) -> None:
        raw = launcher.AUTHORITY_PATH.read_bytes()
        self.assertEqual(launcher.sha256_bytes(raw), launcher.AUTHORITY_SHA256)

    def test_the_authority_is_canonical_json(self) -> None:
        raw = launcher.AUTHORITY_PATH.read_bytes()
        self.assertEqual(
            launcher.canonical_json(json.loads(raw.decode("utf-8"))), raw
        )

    def test_the_build_toolchain_is_not_the_guest_checker_toolchain(self) -> None:
        # The acquired rust-lang-ci nightly compiles submitted proof projects
        # inside the guest.  The launcher is an ordinary workspace crate built by
        # the workspace channel.  Pinning one as the other would misattribute the
        # launcher's provenance.
        identity = _load(
            ROOT
            / "native"
            / "containment"
            / "native-shadow-toolchain-identity-arm64-v1.json"
        )
        authority = _load(launcher.AUTHORITY_PATH)
        self.assertNotEqual(
            authority["toolchain"]["channel"], identity["rust"]["channel"]
        )
        self.assertEqual(authority["toolchain"]["declaredBy"], "rust-toolchain.toml")
        declared = (ROOT / "rust-toolchain.toml").read_text(encoding="utf-8")
        self.assertIn(f'channel = "{authority["toolchain"]["channel"]}"', declared)


if __name__ == "__main__":
    unittest.main()
