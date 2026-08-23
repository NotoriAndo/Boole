#!/usr/bin/env python3
"""Contract tests for the native-shadow production byte manifest compiler."""

from __future__ import annotations

import json
import os
import pathlib
import stat
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from scripts import native_shadow_provenance_manifest as provenance


REQUIRED_CLOSURES = (
    "installed-rust-toolchain-file-manifest",
    "python-interpreter-and-stdlib-file-manifest",
    "system-linker-and-runtime-file-manifest",
)


class NativeShadowProvenanceManifestTests(unittest.TestCase):
    def _set_xattr(self, path: pathlib.Path) -> None:
        original_mode = stat.S_IMODE(path.lstat().st_mode)
        os.chmod(path, original_mode | stat.S_IWUSR)
        if hasattr(os, "setxattr"):
            os.setxattr(path, b"user.boole-test", b"cap")
            os.chmod(path, original_mode)
            return
        completed = subprocess.run(
            ["/usr/bin/xattr", "-w", "user.boole-test", "cap", str(path)],
            check=False,
            text=True,
            capture_output=True,
        )
        os.chmod(path, original_mode)
        self.assertEqual(completed.returncode, 0, completed.stderr)

    def _tree(self, parent: pathlib.Path, name: str) -> pathlib.Path:
        root = parent / name
        (root / "bin").mkdir(parents=True)
        (root / "lib").mkdir()
        (root / "bin" / "tool").write_bytes(b"tool-v1\n")
        os.link(root / "bin" / "tool", root / "bin" / "tool-hard")
        (root / "lib" / "runtime.so").write_bytes(b"runtime-v1\n")
        (root / "bin" / "tool-link").symlink_to("tool")
        os.chmod(root / "bin" / "tool", 0o555)
        os.chmod(root / "lib" / "runtime.so", 0o444)
        return root

    def _inventory(
        self,
        roots: dict[str, pathlib.Path],
        *,
        closure_order: tuple[str, ...] = REQUIRED_CLOSURES,
    ) -> dict[str, object]:
        return {
            "schema": "boole.native-shadow.provenance-inventory.v1",
            "release": "NATIVE-SHADOW-PROVENANCE-SYNTHETIC-TEST",
            "platform": {"os": "linux", "arch": "x86_64"},
            "sourceArtifacts": [
                {"name": "synthetic-root", "sha256": "ab" * 32},
            ],
            "closures": [
                {
                    "name": closure,
                    "roots": [
                        {
                            "logicalPath": f"/authority/{closure}",
                            "sourcePath": str(roots[closure]),
                        }
                    ],
                }
                for closure in closure_order
            ],
        }

    def test_canonical_bytes_are_stable_across_input_and_walk_order(self) -> None:
        with tempfile.TemporaryDirectory() as first_tmp, tempfile.TemporaryDirectory() as second_tmp:
            first_base = pathlib.Path(first_tmp)
            second_base = pathlib.Path(second_tmp)
            first = {
                closure: self._tree(first_base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            second = {
                closure: self._tree(second_base, str(index))
                for index, closure in reversed(tuple(enumerate(REQUIRED_CLOSURES)))
            }

            one = provenance.compile_inventory(self._inventory(first))
            two = provenance.compile_inventory(
                self._inventory(second, closure_order=tuple(reversed(REQUIRED_CLOSURES)))
            )

            self.assertEqual(one, two)
            self.assertTrue(one.endswith(b"\n"))
            self.assertNotIn(b"\r\n", one)

    def test_exact_tree_round_trip_covers_file_directory_and_symlink_without_following(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            raw = provenance.compile_inventory(self._inventory(roots))
            parsed = provenance.verify_inventory(raw, self._inventory(roots))

            kinds = {entry["kind"] for entry in parsed["entries"]}
            self.assertEqual(kinds, {"directory", "file", "symlink"})
            link = next(entry for entry in parsed["entries"] if entry["kind"] == "symlink")
            self.assertEqual(link["target"], "tool")
            self.assertNotIn("sha256", link)

    def test_verify_rejects_missing_extra_digest_mode_owner_and_symlink_target_drift(self) -> None:
        mutators = (
            lambda root: (root / "bin" / "tool").unlink(),
            lambda root: (root / "extra").write_text("extra", encoding="utf-8"),
            lambda root: (
                os.chmod(root / "bin" / "tool", 0o755),
                (root / "bin" / "tool").write_bytes(b"tampered\n"),
            ),
            lambda root: os.chmod(root / "bin" / "tool", 0o500),
            lambda root: (
                (root / "bin" / "tool-link").unlink(),
                (root / "bin" / "tool-link").symlink_to("../lib/runtime.so"),
            ),
            lambda root: (
                (root / "bin" / "tool-hard").unlink(),
                (root / "bin" / "tool-hard").write_bytes(b"tool-v1\n"),
                os.chmod(root / "bin" / "tool-hard", 0o555),
            ),
            lambda root: self._set_xattr(root / "lib" / "runtime.so"),
        )
        for mutate in mutators:
            with self.subTest(mutate=mutate), tempfile.TemporaryDirectory() as tmp:
                base = pathlib.Path(tmp)
                roots = {
                    closure: self._tree(base, str(index))
                    for index, closure in enumerate(REQUIRED_CLOSURES)
                }
                inventory = self._inventory(roots)
                raw = provenance.compile_inventory(inventory)
                mutate(roots[REQUIRED_CLOSURES[0]])
                with self.assertRaises(provenance.ManifestError):
                    provenance.verify_inventory(raw, inventory)

        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            inventory = self._inventory(roots)
            document = json.loads(provenance.compile_inventory(inventory))
            document["entries"][0]["uid"] += 1
            forged = provenance.canonical_json_bytes(document)
            with self.assertRaises(provenance.ManifestError):
                provenance.verify_inventory(forged, inventory)

    def test_verify_rejects_same_size_content_only_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            inventory = self._inventory(roots)
            raw = provenance.compile_inventory(inventory)
            target = roots[REQUIRED_CLOSURES[0]] / "lib" / "runtime.so"
            original_mode = stat.S_IMODE(target.stat().st_mode)
            os.chmod(target, original_mode | stat.S_IWUSR)
            target.write_bytes(b"runtime-v2\n")
            os.chmod(target, original_mode)
            with self.assertRaises(provenance.ManifestError):
                provenance.verify_inventory(raw, inventory)

    def test_directory_swap_to_symlink_during_walk_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            inventory = self._inventory(roots)
            victim = roots[REQUIRED_CLOSURES[0]] / "bin"
            displaced = roots[REQUIRED_CLOSURES[0]] / "bin-before-swap"
            outside = base / "outside"
            outside.mkdir()
            (outside / "secret").write_text("not authority", encoding="utf-8")
            original_open = provenance.os.open
            swapped = False

            def swapping_open(path: object, flags: int, *args: object, **kwargs: object) -> int:
                nonlocal swapped
                if path == "bin" and kwargs.get("dir_fd") is not None and not swapped:
                    swapped = True
                    victim.rename(displaced)
                    victim.symlink_to(outside, target_is_directory=True)
                return original_open(path, flags, *args, **kwargs)

            with mock.patch.object(provenance.os, "open", side_effect=swapping_open):
                with self.assertRaises(provenance.ManifestError):
                    provenance.compile_inventory(inventory)
            self.assertTrue(swapped)

    def test_symlink_escape_dot_component_duplicate_and_unsorted_path_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            inventory = self._inventory(roots)

            inventory["closures"][0]["roots"][0]["logicalPath"] = "/authority/../escape"
            with self.assertRaises(provenance.ManifestError):
                provenance.compile_inventory(inventory)

            inventory = self._inventory(roots)
            inventory["closures"][0]["roots"].append(
                dict(inventory["closures"][0]["roots"][0])
            )
            with self.assertRaises(provenance.ManifestError):
                provenance.compile_inventory(inventory)

            inventory = self._inventory(roots)
            nested = roots[REQUIRED_CLOSURES[0]] / "bin"
            inventory["closures"][0]["roots"].append(
                {
                    "logicalPath": inventory["closures"][0]["roots"][0]["logicalPath"]
                    + "/bin",
                    "sourcePath": str(nested),
                }
            )
            with self.assertRaises(provenance.ManifestError):
                provenance.compile_inventory(inventory)

            inventory = self._inventory(roots)
            (roots[REQUIRED_CLOSURES[0]] / "bin" / "tool-link").unlink()
            (roots[REQUIRED_CLOSURES[0]] / "bin" / "tool-link").symlink_to("../../../escape")
            with self.assertRaises(provenance.ManifestError):
                provenance.compile_inventory(inventory)

            roots[REQUIRED_CLOSURES[0]] = self._tree(base, "replacement")
            inventory = self._inventory(roots)
            document = json.loads(provenance.compile_inventory(inventory))
            document["entries"] = list(reversed(document["entries"]))
            noncanonical = json.dumps(document, sort_keys=True).encode() + b"\n"
            with self.assertRaises(provenance.ManifestError):
                provenance.verify_inventory(noncanonical, inventory)

    def test_relative_symlink_cannot_confuse_unrelated_physical_root_mapping(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            (roots[REQUIRED_CLOSURES[0]] / "escape-link").symlink_to("../evil")
            physical_escape = base / "evil"
            physical_escape.mkdir()
            (physical_escape / "secret").write_text("outside", encoding="utf-8")
            unrelated = self._tree(base, "unrelated")
            inventory = self._inventory(roots)
            inventory["closures"][0]["roots"] = [
                {
                    "logicalPath": "/a",
                    "sourcePath": str(roots[REQUIRED_CLOSURES[0]]),
                },
                {
                    "logicalPath": "/evil",
                    "sourcePath": str(unrelated),
                },
            ]
            with self.assertRaises(provenance.ManifestError):
                provenance.compile_inventory(inventory)

    def test_absolute_symlink_target_is_rejected_even_when_logical_root_exists(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            physical_escape = base / "absolute-evil"
            physical_escape.mkdir()
            (physical_escape / "secret").write_text("outside", encoding="utf-8")
            (roots[REQUIRED_CLOSURES[0]] / "absolute-link").symlink_to(
                physical_escape, target_is_directory=True
            )
            unrelated = self._tree(base, "absolute-unrelated")
            inventory = self._inventory(roots)
            inventory["closures"][0]["roots"] = [
                {
                    "logicalPath": "/a",
                    "sourcePath": str(roots[REQUIRED_CLOSURES[0]]),
                },
                {
                    "logicalPath": str(physical_escape),
                    "sourcePath": str(unrelated),
                },
            ]
            with self.assertRaises(provenance.ManifestError):
                provenance.compile_inventory(inventory)

    def test_one_closed_tree_may_serve_multiple_closures_without_duplicate_entries(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            inventory = self._inventory(roots)
            shared = dict(inventory["closures"][0]["roots"][0])
            inventory["closures"][1]["roots"] = [shared]
            document = json.loads(provenance.compile_inventory(inventory))
            shared_entries = [
                entry
                for entry in document["entries"]
                if entry["logicalPath"].startswith(shared["logicalPath"])
            ]
            self.assertTrue(shared_entries)
            self.assertTrue(
                all(
                    entry["closures"] == list(REQUIRED_CLOSURES[:2])
                    for entry in shared_entries
                )
            )

    def test_manifest_requires_exactly_three_nonempty_closures(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            inventory = self._inventory(roots)
            inventory["closures"].pop()
            with self.assertRaises(provenance.ManifestError):
                provenance.compile_inventory(inventory)

            inventory = self._inventory(roots)
            inventory["closures"][1]["roots"] = []
            with self.assertRaises(provenance.ManifestError):
                provenance.compile_inventory(inventory)

            inventory = self._inventory(roots)
            inventory["closures"][2]["name"] = "unexpected-closure"
            with self.assertRaises(provenance.ManifestError):
                provenance.compile_inventory(inventory)

            inventory = self._inventory(roots)
            inventory["closures"][0]["name"] = []
            with self.assertRaises(provenance.ManifestError):
                provenance.compile_inventory(inventory)

    def test_manifest_never_uses_device_inode_mtime_or_mutable_source_root_as_identity(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            document = json.loads(provenance.compile_inventory(self._inventory(roots)))
            self.assertEqual(
                document["metadataPolicy"]["extendedAttributes"],
                "canonical-name-and-value-hex",
            )
            self.assertEqual(
                document["metadataPolicy"]["excludedMachineLocalFields"],
                ["device", "inode", "mtime", "ctime"],
            )
            identity_fields = document["metadataPolicy"]["identityFields"]
            for forbidden in ("sourcePath", "device", "inode", "mtime", "ctime"):
                self.assertNotIn(forbidden, identity_fields)
                for entry in document["entries"]:
                    self.assertNotIn(forbidden, entry)

    def test_activation_is_literal_false_and_scaffold_cannot_claim_complete(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            document = json.loads(provenance.compile_inventory(self._inventory(roots)))
            self.assertIs(document["activationAllowed"], False)
            self.assertIs(document["productionByteProvenanceComplete"], False)
            self.assertEqual(document["authorityStatus"], "SCAFFOLD-NOT-ACTIVATABLE")

    def test_cli_generate_then_verify_tiny_synthetic_root(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            roots = {
                closure: self._tree(base, str(index))
                for index, closure in enumerate(REQUIRED_CLOSURES)
            }
            inventory_path = base / "inventory.json"
            manifest_path = base / "manifest.json"
            inventory_path.write_text(
                json.dumps(self._inventory(roots), indent=2) + "\n", encoding="utf-8"
            )
            script = pathlib.Path(provenance.__file__).resolve()
            generated = subprocess.run(
                [sys.executable, str(script), "generate", "--inventory", str(inventory_path),
                 "--output", str(manifest_path)],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(generated.returncode, 0, generated.stderr)
            verified = subprocess.run(
                [sys.executable, str(script), "verify", "--inventory", str(inventory_path),
                 "--manifest", str(manifest_path)],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(verified.returncode, 0, verified.stderr)

            malformed = self._inventory(roots)
            malformed["closures"][0]["name"] = []
            inventory_path.write_text(
                json.dumps(malformed, indent=2) + "\n", encoding="utf-8"
            )
            rejected = subprocess.run(
                [sys.executable, str(script), "generate", "--inventory", str(inventory_path),
                 "--output", str(manifest_path)],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(rejected.returncode, 2, rejected.stderr)
            self.assertNotIn("Traceback", rejected.stderr)


if __name__ == "__main__":
    unittest.main()
