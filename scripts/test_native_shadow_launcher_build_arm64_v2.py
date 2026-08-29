"""RED/GREEN contract for the launcher-v2 reproducible overlay build.

The old launcher build remains historical evidence.  The successor therefore
builds from the sealed v1 tree plus complete replacement files in a temporary
export.  It also owns a fresh Cargo home and proves that no producer path is
left in the ELF before a digest can be sealed.
"""

import importlib
import json
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest


def launcher_v2():
    return importlib.import_module("scripts.native_shadow_launcher_build_arm64_v2")


class EnvironmentTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = launcher_v2()
        self.authority = {
            "build": {
                "rustflags": [
                    "--remap-path-prefix",
                    "{sourceRoot}=/boole/launcher-build",
                    "--remap-path-prefix",
                    "{cargoHome}=/boole/cargo-home",
                ]
            },
            "determinism": {
                "declaredEnvironment": {
                    "CARGO_INCREMENTAL": "0",
                    "CARGO_TERM_COLOR": "never",
                    "LANG": "C",
                    "LC_ALL": "C",
                    "SOURCE_DATE_EPOCH": None,
                    "TZ": "UTC",
                }
            },
        }

    def test_the_source_and_cargo_roots_are_both_remapped(self) -> None:
        environment = self.module.build_environment(
            self.authority,
            source_root=pathlib.Path("/tmp/source-a"),
            cargo_home=pathlib.Path("/tmp/cargo-a"),
            ambient={"HOME": "/home/runner", "RUSTUP_HOME": "/rustup", "PATH": "/bin"},
        )
        flags = environment["CARGO_ENCODED_RUSTFLAGS"].split("\x1f")
        self.assertEqual(
            flags,
            [
                "--remap-path-prefix",
                "/tmp/source-a=/boole/launcher-build",
                "--remap-path-prefix",
                "/tmp/cargo-a=/boole/cargo-home",
            ],
        )
        self.assertEqual(environment["CARGO_HOME"], "/tmp/cargo-a")

    def test_source_date_epoch_stays_unset(self) -> None:
        environment = self.module.build_environment(
            self.authority,
            source_root=pathlib.Path("/tmp/source-a"),
            cargo_home=pathlib.Path("/tmp/cargo-a"),
            ambient={"HOME": "/home/runner", "RUSTUP_HOME": "/rustup", "PATH": "/bin"},
        )
        self.assertNotIn("SOURCE_DATE_EPOCH", environment)

    def test_relative_source_or_cargo_roots_are_refused(self) -> None:
        for source, cargo in (
            (pathlib.Path("relative"), pathlib.Path("/tmp/cargo")),
            (pathlib.Path("/tmp/source"), pathlib.Path("relative")),
        ):
            with self.subTest(source=source, cargo=cargo):
                with self.assertRaises(self.module.LauncherV2BuildError):
                    self.module.build_environment(
                        self.authority,
                        source_root=source,
                        cargo_home=cargo,
                        ambient={"HOME": "/home/runner", "RUSTUP_HOME": "/rustup"},
                    )


class ProducerPathTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = launcher_v2()
        self.roots = {
            "source-root": pathlib.Path("/tmp/source-a"),
            "cargo-home": pathlib.Path("/tmp/cargo-a"),
            "repository-root": pathlib.Path("/work/Boole"),
            "ambient-home": pathlib.Path("/home/runner"),
            "rustup-home": pathlib.Path("/opt/rustup"),
        }

    def test_each_producer_path_is_refused_in_the_artifact(self) -> None:
        for label, path in self.roots.items():
            with self.subTest(label=label):
                with self.assertRaisesRegex(self.module.LauncherV2BuildError, label):
                    self.module.assert_no_producer_paths(
                        b"ELF\0" + str(path).encode() + b"/dependency.rs\0",
                        self.roots,
                    )

    def test_the_historical_runner_cargo_path_is_refused(self) -> None:
        artifact = (
            b"ELF\0/home/runner/.cargo/registry/src/index.crates.io-123/sha2/src/lib.rs\0"
        )
        with self.assertRaises(self.module.LauncherV2BuildError):
            self.module.assert_no_producer_paths(artifact, self.roots)

    def test_canonical_remapped_paths_are_allowed(self) -> None:
        artifact = (
            b"ELF\0/boole/launcher-build/src/main.rs\0"
            b"/boole/cargo-home/registry/src/sha2/src/lib.rs\0"
        )
        self.assertEqual(
            self.module.assert_no_producer_paths(artifact, self.roots),
            {label: 0 for label in self.roots},
        )

    def test_raw_paths_are_not_returned_in_the_scan_result(self) -> None:
        result = self.module.assert_no_producer_paths(b"ELF\0clean\0", self.roots)
        self.assertEqual(set(result), set(self.roots))
        self.assertTrue(all(value == 0 for value in result.values()))
        self.assertNotIn("/home/runner", repr(result))


class OverlayTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = launcher_v2()

    def row(self, source: pathlib.Path, destination: str) -> dict:
        raw = source.read_bytes()
        return {
            "destination": destination,
            "sha256": self.module.sha256_bytes(raw),
            "sizeBytes": len(raw),
            "source": source.name,
        }

    def test_complete_file_overlay_is_applied_only_inside_the_export(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            overlay = root / "overlay"
            exported = root / "exported"
            overlay.mkdir()
            exported.mkdir()
            (overlay / "new.rs").write_bytes(b"pub const V2: bool = true;\n")
            row = self.row(overlay / "new.rs", "crate/src/new.rs")
            self.module.apply_complete_file_overlay([row], overlay, exported)
            self.assertEqual(
                (exported / "crate/src/new.rs").read_bytes(),
                b"pub const V2: bool = true;\n",
            )
            self.assertEqual((overlay / "new.rs").read_bytes(), b"pub const V2: bool = true;\n")

    def test_overlay_traversal_and_absolute_destinations_are_refused(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            overlay = root / "overlay"
            exported = root / "exported"
            overlay.mkdir()
            exported.mkdir()
            (overlay / "new.rs").write_bytes(b"x")
            for destination in ("../escape.rs", "/absolute.rs"):
                with self.subTest(destination=destination):
                    row = self.row(overlay / "new.rs", destination)
                    with self.assertRaises(self.module.LauncherV2BuildError):
                        self.module.apply_complete_file_overlay([row], overlay, exported)

    def test_duplicate_destinations_are_refused(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            overlay = root / "overlay"
            exported = root / "exported"
            overlay.mkdir()
            exported.mkdir()
            (overlay / "a.rs").write_bytes(b"a")
            (overlay / "b.rs").write_bytes(b"b")
            rows = [
                self.row(overlay / "a.rs", "crate/src/lib.rs"),
                self.row(overlay / "b.rs", "crate/src/lib.rs"),
            ]
            with self.assertRaises(self.module.LauncherV2BuildError):
                self.module.apply_complete_file_overlay(rows, overlay, exported)

    def test_overlay_digest_and_size_drift_are_refused(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            overlay = root / "overlay"
            exported = root / "exported"
            overlay.mkdir()
            exported.mkdir()
            (overlay / "new.rs").write_bytes(b"right")
            row = self.row(overlay / "new.rs", "crate/src/lib.rs")
            for field, value in (("sha256", "0" * 64), ("sizeBytes", 99)):
                with self.subTest(field=field):
                    changed = dict(row)
                    changed[field] = value
                    with self.assertRaises(self.module.LauncherV2BuildError):
                        self.module.apply_complete_file_overlay([changed], overlay, exported)

    def test_an_overlay_source_symlink_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            root = pathlib.Path(raw)
            overlay = root / "overlay"
            exported = root / "exported"
            overlay.mkdir()
            exported.mkdir()
            (overlay / "target.rs").write_bytes(b"target")
            (overlay / "link.rs").symlink_to("target.rs")
            row = {
                "destination": "crate/src/lib.rs",
                "sha256": self.module.sha256_bytes(b"target"),
                "sizeBytes": 6,
                "source": "link.rs",
            }
            with self.assertRaises(self.module.LauncherV2BuildError):
                self.module.apply_complete_file_overlay([row], overlay, exported)

    def test_an_exact_source_replacement_must_match_once(self) -> None:
        original = b"one\nold block\nthree\n"
        changed = self.module.apply_exact_source_replacement(
            original,
            before=b"old block\n",
            after=b"new block\n",
            label="drop verifier",
        )
        self.assertEqual(changed, b"one\nnew block\nthree\n")
        for bad in (b"one\nthree\n", b"old block\nold block\n"):
            with self.subTest(bad=bad):
                with self.assertRaises(self.module.LauncherV2BuildError):
                    self.module.apply_exact_source_replacement(
                        bad,
                        before=b"old block\n",
                        after=b"new block\n",
                        label="drop verifier",
                    )


class DeterminismTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = launcher_v2()

    def test_two_identical_artifacts_pass_and_are_scanned(self) -> None:
        calls = []

        def builder(index):
            calls.append(index)
            return b"same"

        result = self.module.compare_two_artifacts(
            builder,
            {"ambient-home": pathlib.Path("/home/runner")},
        )
        self.assertEqual(calls, [0, 1])
        self.assertEqual(result["buildCount"], 2)

    def test_byte_or_length_differences_are_refused_without_a_retry(self) -> None:
        for outputs in ((b"a", b"b"), (b"a", b"aa")):
            calls = []

            def builder(index):
                calls.append(index)
                return outputs[index]

            with self.subTest(outputs=outputs):
                with self.assertRaises(self.module.LauncherV2BuildError):
                    self.module.compare_two_artifacts(builder, {})
                self.assertEqual(calls, [0, 1])


class FrozenOverlayTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = launcher_v2()
        self.record = self.module.load_source_overlay()

    def test_source_overlay_record_is_pinned_and_grants_no_execution(self) -> None:
        self.assertEqual(
            self.module.sha256_bytes(self.module.SOURCE_OVERLAY_PATH.read_bytes()),
            self.module.SOURCE_OVERLAY_SHA256,
        )
        self.assertFalse(self.record["imageProductionAuthorisation"])
        self.assertFalse(self.record["bootAuthorisation"])
        self.assertFalse(self.record["activationAllowed"])

    def test_v1_live_sources_still_match_the_historical_authority(self) -> None:
        authority = self.module.v1.load_authority()
        self.assertEqual(self.module.v1.verify_sources(authority), [])
        self.assertEqual(
            self.record["baseV1Authority"]["sha256"],
            self.module.v1.AUTHORITY_SHA256,
        )

    def test_real_overlay_materializes_the_exact_successor_files(self) -> None:
        with tempfile.TemporaryDirectory() as raw:
            exported = pathlib.Path(raw)
            destinations = [
                row["destination"] for row in self.record["completeFiles"]
            ] + [row["destination"] for row in self.record["exactReplacements"]]
            for relative in destinations:
                source = self.module.REPO_ROOT / relative
                if source.is_file():
                    destination = exported / relative
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(source, destination)
            fixture = self.record["sharedConsoleFixture"]["path"]
            fixture_destination = exported / fixture
            fixture_destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(self.module.REPO_ROOT / fixture, fixture_destination)
            result = self.module.apply_source_overlay(self.record, exported_root=exported)
            self.assertEqual(
                result["completeFiles"], len(self.record["completeFiles"])
            )
            self.assertEqual(
                result["exactReplacements"], len(self.record["exactReplacements"])
            )
            for row in self.record["completeFiles"]:
                raw_file = (exported / row["destination"]).read_bytes()
                self.assertEqual(len(raw_file), row["sizeBytes"])
                self.assertEqual(self.module.sha256_bytes(raw_file), row["sha256"])
            for row in self.record["exactReplacements"]:
                raw_file = (exported / row["destination"]).read_bytes()
                self.assertEqual(len(raw_file), row["successorSizeBytes"])
                self.assertEqual(
                    self.module.sha256_bytes(raw_file), row["successorSha256"]
                )

    def test_overlay_manifest_cannot_substitute_a_different_base_generation(self) -> None:
        changed = json.loads(json.dumps(self.record))
        changed["baseV1Authority"]["sha256"] = "0" * 64
        with self.assertRaises(self.module.LauncherV2BuildError):
            self.module.validate_source_overlay(changed)

    def test_overlay_status_rationale_and_source_properties_are_exact(self) -> None:
        mutations = [
            ("status", "weaker"),
            ("whyOverlay", ""),
            ("sourceProperties", {}),
        ]
        for field, value in mutations:
            with self.subTest(field=field):
                changed = json.loads(json.dumps(self.record))
                changed[field] = value
                with self.assertRaises(self.module.LauncherV2BuildError):
                    self.module.validate_source_overlay(changed)

    def test_readiness_is_emitted_only_after_the_fixed_listener_is_bound(self) -> None:
        destinations = {
            row["destination"] for row in self.record["exactReplacements"]
        }
        self.assertIn(
            "crates/boole-native-shadow-launcher/src/active_execution/mod.rs",
            destinations,
        )
        active = (
            self.module.REPO_ROOT
            / "native/launcher-v2-overlay/active-execution-after.rs.txt"
        ).read_text(encoding="utf-8")
        bind = active.index("let mut listener = bind_listener_in_directory(")
        notify = active.index("crate::console_evidence::emit(&mut stdout.lock(), &records)")
        accept = active.index("let qualification_stream = listener.accept_one()?")
        self.assertLess(bind, notify)
        self.assertLess(notify, accept)

        main = (
            self.module.REPO_ROOT
            / "native/launcher-v2-overlay/boole-native-shadow-launcher.rs"
        ).read_text(encoding="utf-8")
        self.assertIn(
            "serve_qualified_three_fixed_unix_executions_with_listener_bound_console_evidence",
            main,
        )
        callback = main.index(
            "serve_qualified_three_fixed_unix_executions_with_listener_bound_console_evidence("
        )
        self.assertNotIn("emit(&mut stdout.lock(), &records)", main)
        self.assertGreater(callback, main.index("let records = ["))
        self.assertIn("records: [crate::console_evidence::Record; 4]", active)
        self.assertIn("ListenerBoundConsoleEvidence { reason: String }", active)


class BuildAuthorityTests(unittest.TestCase):
    def setUp(self) -> None:
        self.module = launcher_v2()
        self.authority = self.module.load_authority()

    def test_authority_pins_the_overlay_and_two_path_remaps(self) -> None:
        self.assertEqual(
            self.authority["sourceOverlay"]["sha256"],
            self.module.SOURCE_OVERLAY_SHA256,
        )
        self.assertEqual(
            self.authority["build"]["rustflags"],
            [
                "--remap-path-prefix",
                "{sourceRoot}=/boole/launcher-build",
                "--remap-path-prefix",
                "{cargoHome}=/boole/cargo-home",
            ],
        )
        self.assertTrue(self.authority["determinism"]["freshCargoHomePerBuild"])

    def test_authority_runs_the_overlay_rust_tests_on_real_linux_arm64(self) -> None:
        self.assertEqual(
            self.authority["build"]["testCommand"],
            [
                "cargo",
                "test",
                "--locked",
                "--offline",
                "--target",
                "aarch64-unknown-linux-gnu",
                "-p",
                "boole-native-shadow-launcher",
                "--features",
                "linux-arm64-authority",
                "--lib",
                "--bins",
            ],
        )

    def test_authority_keeps_timestamp_suppression_and_postprocessing_forbidden(self) -> None:
        self.assertIsNone(
            self.authority["determinism"]["declaredEnvironment"]["SOURCE_DATE_EPOCH"]
        )
        self.assertTrue(self.authority["determinism"]["forbidTimestampSuppression"])
        self.assertTrue(self.authority["determinism"]["forbidBinaryPostprocessing"])
        self.assertEqual(self.authority["build"]["postprocessCommand"], None)

    def test_authority_and_generator_are_mutually_pinned_without_a_cycle(self) -> None:
        self.assertEqual(
            self.module.sha256_bytes(self.module.AUTHORITY_PATH.read_bytes()),
            self.module.AUTHORITY_SHA256,
        )
        self.assertEqual(
            self.module.launcher_v2_generator_sha256(self.module.TOOL_PATH.read_bytes()),
            self.authority["generator"]["sha256"],
        )

    def test_authority_grants_no_image_boot_or_activation(self) -> None:
        self.assertFalse(self.authority["activationAllowed"])
        self.assertFalse(self.authority["bootableClaim"])
        self.assertTrue(all(value is False for value in self.authority["boundaries"].values()))

    def test_authority_boundary_names_are_exact_not_merely_all_false(self) -> None:
        for mutate in ("delete", "invent"):
            with self.subTest(mutate=mutate):
                changed = json.loads(json.dumps(self.authority))
                if mutate == "delete":
                    del changed["boundaries"]["bootAuthority"]
                else:
                    changed["boundaries"]["inventedBoundary"] = False
                with self.assertRaises(self.module.LauncherV2BuildError):
                    self.module.validate_authority(changed)

    def test_result_records_zero_counts_without_raw_producer_paths(self) -> None:
        result = self.module.build_result(
            self.authority,
            built={
                "buildCount": 2,
                "producerPathHits": {
                    "ambient-home": 0,
                    "cargo-home": 0,
                    "repository-root": 0,
                    "rustup-home": 0,
                    "source-root": 0,
                },
                "sha256": "a" * 64,
                "sizeBytes": 123,
            },
            identity={"cargo": "cargo 1.95.0", "host": "aarch64-unknown-linux-gnu", "rustc": "rustc 1.95.0"},
        )
        encoded = json.dumps(result, sort_keys=True)
        self.assertNotIn("/home/runner", encoded)
        self.assertNotIn("/tmp/", encoded)
        self.assertEqual(set(result["producerPathHits"].values()), {0})
        self.assertEqual(result["overlaySourceTestRuns"], 2)

    def test_tracked_cli_can_check_the_authority_from_the_repository_root(self) -> None:
        completed = subprocess.run(
            [sys.executable, str(self.module.TOOL_PATH), "--check"],
            cwd=str(self.module.REPO_ROOT),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
        self.assertEqual(completed.returncode, 0, completed.stderr)
        self.assertIn(self.module.AUTHORITY_SHA256, completed.stdout)

    def test_the_ci_prints_but_refuses_an_untracked_candidate(self) -> None:
        workflow = (self.module.REPO_ROOT / ".github/workflows/ci.yml").read_text(
            encoding="utf-8"
        )
        job = workflow.split("  native-shadow-launcher-build-arm64-v2:\n", 1)[1]
        job = job.split("\n  self-test:\n", 1)[0]
        printed = job.index('cat "$result"')
        tracked = job.index('git ls-files --error-unmatch -- "$result"')
        refused = job.index("exit 1", tracked)
        self.assertLess(printed, tracked)
        self.assertLess(tracked, refused)
        self.assertNotIn("continue-on-error", job)
        self.assertNotIn("|| true", job)


if __name__ == "__main__":
    unittest.main()
