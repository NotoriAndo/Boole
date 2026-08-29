"""Tests the successor path/content reconciliation without booting the guest."""

from __future__ import annotations

import collections
import hashlib
import importlib
import json
import pathlib
import sys
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]


def module():
    if str(REPO / "scripts") not in sys.path:
        sys.path.insert(0, str(REPO / "scripts"))
    return importlib.import_module(
        "native_shadow_mac3_guest_secret_path_content_reconcile_arm64_v1"
    )


class GuestSecretPathContentReconcilePureTests(unittest.TestCase):
    def setUp(self) -> None:
        self.mod = module()

    def test_slt_parser_keeps_regular_symlink_directory_and_journal_entries(self) -> None:
        text = """
Path = /archive
Type = Ext
Physical Size = 8192

----------
Path = etc
Folder = +
Size =
Mode = drwxr-xr-x
Symbolic Link =
User = 0
Group = 0

Path = bin
Folder = +
Size = 7
Mode = lrwxrwxrwx
Symbolic Link = usr/bin
User = 0
Group = 0

Path = etc/passwd
Folder = -
Size = 9
Mode = -r--r--r--
Symbolic Link =
User = 0
Group = 0

Path = [SYS]/Journal
Folder = -
Size = 4096
Mode = -rw-------
Symbolic Link =
User = 0
Group = 0
"""
        entries = self.mod.parse_slt(text)
        self.assertEqual([row.path for row in entries], ["etc", "bin", "etc/passwd", "[SYS]/Journal"])
        self.assertEqual([row.kind for row in entries], ["directory", "symlink", "file", "journal"])
        self.assertEqual([row.size for row in entries], [0, 7, 9, 4096])

    def test_duplicate_absolute_and_parent_paths_are_refused(self) -> None:
        for path in (
            "/etc/passwd",
            "../etc/passwd",
            "etc/../passwd",
            "etc//passwd",
            "etc/./passwd",
            "etc/passwd/",
            "etc/pass\nwd",
            "",
        ):
            with self.subTest(path=path), self.assertRaises(self.mod.RefusedError):
                self.mod.validate_logical_paths([path])
        with self.assertRaises(self.mod.RefusedError):
            self.mod.validate_logical_paths(["etc/passwd", "etc/passwd"])
        self.mod.validate_logical_paths(
            [r"usr/lib/systemd/system/system-systemd\x2dcryptsetup.slice"]
        )

    def test_the_staging_manifest_removes_exactly_the_four_generated_entries(self) -> None:
        paths = [
            "etc",
            "usr",
            "[SYS]/Journal",
            "lost+found",
            "usr/libexec/boole",
            "usr/libexec/boole/boole-native-shadow-launcher",
        ]
        base, digest = self.mod.staging_path_manifest(paths)
        self.assertEqual(base, ["etc", "usr"])
        self.assertEqual(
            digest,
            self.mod.sha256_bytes(b"etc\nusr\n"),
        )
        generated = (path for path in paths)
        generated_base, generated_digest = self.mod.staging_path_manifest(generated)
        self.assertEqual(generated_base, base)
        self.assertEqual(generated_digest, digest)

    def test_raw_and_logical_signature_multisets_must_match_exactly(self) -> None:
        signature = ("mnemonic", "a" * 64, 17)
        doubled = collections.Counter({signature: 2})
        report = self.mod.reconcile_signature_counters(doubled, doubled, collections.Counter())
        self.assertEqual(report["rawHits"], 2)
        self.assertEqual(report["logicalFileHits"], 2)
        self.assertEqual(report["journalHits"], 0)
        self.assertEqual(report["unmappedRawHits"], 0)

        with self.assertRaises(self.mod.RefusedError):
            self.mod.reconcile_signature_counters(
                doubled,
                collections.Counter({signature: 1}),
                collections.Counter(),
            )
        with self.assertRaises(self.mod.RefusedError):
            self.mod.reconcile_signature_counters(
                doubled,
                collections.Counter(),
                doubled,
            )
        for invalid in (
            collections.Counter({signature: 0}),
            collections.Counter({signature: -1}),
            collections.Counter({signature: True}),
            collections.Counter({("mnemonic", "bad-digest", 17): 1}),
            collections.Counter({("mnemonic", "a" * 64, 4096): 1}),
        ):
            with self.subTest(invalid=invalid), self.assertRaises(self.mod.RefusedError):
                self.mod.reconcile_signature_counters(
                    invalid,
                    invalid,
                    collections.Counter(),
                )

    def test_every_raw_hit_must_have_one_physical_logical_owner(self) -> None:
        hit = self.mod.PhysicalHit(
            marker="mnemonic",
            raw_offset=8193,
            block_sha256="a" * 64,
            block_position=1,
        )
        report = self.mod.reconcile_physical_owners(
            [hit],
            {
                hit: self.mod.LogicalOwner(
                    inode=42,
                    paths=("usr/lib/libexample.so",),
                )
            },
            journal_hits=[],
        )
        self.assertEqual(report["attributedRawHits"], 1)
        hardlinked = self.mod.reconcile_physical_owners(
            [hit],
            {
                hit: self.mod.LogicalOwner(
                    inode=42,
                    paths=("usr/lib/libexample.so", "usr/lib/libexample-hardlink.so"),
                )
            },
            journal_hits=[],
        )
        self.assertEqual(hardlinked["attributedRawHits"], 1)

        for owner in (
            self.mod.LogicalOwner(inode=42, paths=()),
            self.mod.LogicalOwner(inode=0, paths=("usr/a",)),
        ):
            with self.subTest(owner=owner), self.assertRaises(self.mod.RefusedError):
                self.mod.reconcile_physical_owners(
                    [hit],
                    {hit: owner},
                    journal_hits=[],
                )

    def test_only_sealed_upstream_namespaces_are_classified_as_references(self) -> None:
        examples = {
            "boot/System.map-6.8.0-31-generic": "boot/System.map-6.8.0-31-generic",
            "usr/lib/libc.so": "usr/lib/libc.so",
            "opt/boole/native-checker-toolchain/bin/cargo": "opt/boole/native-checker-toolchain/bin/cargo",
            "var/lib/boole/native-shadow/runtime-rootfs/usr/lib/libc.so": "usr/lib/libc.so",
        }
        for path, canonical in examples.items():
            with self.subTest(path=path):
                row = self.mod.classify_candidate_path(path)
                self.assertEqual(row["classification"], "UPSTREAM-NAMESPACE-CANDIDATE")
                self.assertEqual(row["canonicalPath"], canonical)

        for path in (
            "etc/passwd",
            "usr/../etc/shadow",
            "usr/libexec/boole/secret",
            "usr/libexec/boole/boole-native-shadow-launcher",
            "var/lib/boole/native-shadow/ROOTFS-CONTENT-MANIFEST.json",
            "[SYS]/Journal",
        ):
            with self.subTest(path=path), self.assertRaises(self.mod.RefusedError):
                self.mod.classify_candidate_path(path)

    def test_candidate_needs_exact_sealed_path_kind_and_digest_membership(self) -> None:
        payload = b"ordinary upstream text"
        digest = hashlib.sha256(payload).hexdigest()
        observed = self.mod.ObservedContent(
            path="usr/share/doc/example.txt",
            kind="file",
            sha256=digest,
            symlink_target="",
        )
        sealed = {
            observed.path: self.mod.SealedContent(
                kind="file",
                sha256=digest,
                symlink_target="",
                source="boot-source-manifest-v2",
            )
        }
        bound = self.mod.bind_candidate_to_sealed_source(observed, sealed)
        self.assertEqual(bound["classification"], "SEALED-EXPECTED-FILE-CONTENT")
        self.assertEqual(bound["source"], "boot-source-manifest-v2")

        for changed in (
            self.mod.ObservedContent(observed.path, "file", "b" * 64, ""),
            self.mod.ObservedContent(observed.path, "symlink", digest, "elsewhere"),
            self.mod.ObservedContent("usr/share/doc/unsealed.txt", "file", digest, ""),
            self.mod.ObservedContent("boot/host-key", "file", digest, ""),
        ):
            with self.subTest(changed=changed), self.assertRaises(self.mod.RefusedError):
                self.mod.bind_candidate_to_sealed_source(changed, sealed)

    def test_symlink_binding_uses_the_sealed_target_without_inventing_a_digest(self) -> None:
        observed = self.mod.ObservedContent(
            path="usr/bin/example",
            kind="symlink",
            sha256="",
            symlink_target="../lib/example",
        )
        sealed = {
            observed.path: self.mod.SealedContent(
                kind="symlink",
                sha256="",
                symlink_target="../lib/example",
                source="boot-source-manifest-v2",
            )
        }
        bound = self.mod.bind_candidate_to_sealed_source(observed, sealed)
        self.assertEqual(bound["classification"], "SEALED-EXPECTED-SYMLINK")
        self.assertEqual(bound["source"], "boot-source-manifest-v2")

        for changed in (
            self.mod.ObservedContent(observed.path, "symlink", "a" * 64, observed.symlink_target),
            self.mod.ObservedContent(observed.path, "symlink", "", "../lib/other"),
        ):
            with self.subTest(changed=changed), self.assertRaises(self.mod.RefusedError):
                self.mod.bind_candidate_to_sealed_source(changed, sealed)

    def test_symlink_targets_are_lexically_safe_and_scanned(self) -> None:
        self.assertEqual(
            self.mod.validate_symlink_target("usr/bin/example", "../lib/example"),
            "usr/lib/example",
        )
        self.assertEqual(
            self.mod.validate_symlink_target("usr/sbin/rmt", "/etc/rmt"),
            "etc/rmt",
        )
        self.assertEqual(
            self.mod.validate_symlink_target(
                "var/lib/boole/native-shadow/runtime-rootfs/usr/sbin/rmt",
                "/usr/lib/rmt",
            ),
            "var/lib/boole/native-shadow/runtime-rootfs/usr/lib/rmt",
        )
        for target in ("../../../host", "bad\ntarget", ""):
            with self.subTest(target=target), self.assertRaises(self.mod.RefusedError):
                self.mod.validate_symlink_target("usr/bin/example", target)
        self.assertEqual(
            self.mod.forbidden_symlink_target_hits(
                {"home/user/key-link": "../.boole/keys/key.json"}
            ),
            ["home/user/key-link -> home/.boole/keys/key.json"],
        )

    def test_expected_table_adds_only_the_sealed_launcher_and_lost_found(self) -> None:
        entries = {
            "etc": {
                "path": "etc",
                "kind": "directory",
                "mode": 0o755,
                "uid": 0,
                "gid": 0,
            },
            "etc/example": {
                "path": "etc/example",
                "kind": "file",
                "mode": 0o444,
                "uid": 0,
                "gid": 0,
                "raw": b"example\n",
            },
            "bin": {
                "path": "bin",
                "kind": "symlink",
                "mode": 0o777,
                "uid": 0,
                "gid": 0,
                "target": "usr/bin",
                "resolvedTarget": "usr/bin",
            },
        }
        table = self.mod.complete_expected_table(
            entries,
            launcher_path="usr/libexec/boole/boole-native-shadow-launcher",
            launcher_size=2_006_632,
            launcher_sha256="a" * 64,
            enforce_sealed_counts=False,
        )
        self.assertEqual(
            set(table) - set(entries),
            {
                "usr/libexec/boole",
                "usr/libexec/boole/boole-native-shadow-launcher",
                "lost+found",
            },
        )
        self.assertEqual(table["etc/example"].sha256, hashlib.sha256(b"example\n").hexdigest())
        self.assertEqual(table["bin"].symlink_target, "usr/bin")
        self.assertEqual(table["lost+found"].mode, 0o700)
        manifest = self.mod.expected_table_manifest(table)
        self.assertEqual(manifest["entries"], 6)
        self.assertEqual(manifest["byKind"], {"directory": 3, "file": 2, "symlink": 1})

    def test_preserved_image_locator_uses_replica_one_relative_row_only(self) -> None:
        qualification = {
            "subject": {
                "images": [
                    {
                        "archivePath": "successor-outputs-1/guest-root-disk",
                        "bytes": 99,
                        "name": "guest-root-disk",
                        "replica": 1,
                        "sha256": "a" * 64,
                        "used": True,
                    }
                ]
            }
        }
        preservation = {
            "preservedFiles": [
                {
                    "path": "successor-outputs-1/guest-root-disk",
                    "bytes": 99,
                    "sha256": "a" * 64,
                }
            ]
        }
        row = self.mod.resolve_preserved_root_disk(qualification, preservation)
        self.assertEqual(row["relativePath"], "successor-outputs-1/guest-root-disk")
        self.assertEqual(row["sizeBytes"], 99)

        for changed in (
            {**qualification, "subject": {"images": []}},
            {
                **qualification,
                "subject": {
                    "images": qualification["subject"]["images"]
                    + [dict(qualification["subject"]["images"][0])]
                },
            },
        ):
            with self.subTest(changed=changed), self.assertRaises(self.mod.RefusedError):
                self.mod.resolve_preserved_root_disk(changed, preservation)
        traversal = {
            "subject": {
                "images": [
                    {
                        **qualification["subject"]["images"][0],
                        "archivePath": "../guest-root-disk",
                    }
                ]
            }
        }
        with self.assertRaises(self.mod.RefusedError):
            self.mod.resolve_preserved_root_disk(traversal, preservation)

    def test_frozen_marker_table_needs_the_explicit_private_marker(self) -> None:
        private = b"/Users/example"
        rows = [
            {
                "id": "host-home-directory",
                "needle": None,
                "needleBytes": len(private),
                "needleSha256": hashlib.sha256(private).hexdigest(),
                "tier": "host-identity",
            },
            {
                "id": "netrc-credentials-file",
                "needle": ".netrc",
                "needleBytes": 6,
                "needleSha256": hashlib.sha256(b".netrc").hexdigest(),
                "tier": "secret-shape",
            },
        ]
        markers = self.mod.frozen_markers(rows, private_host_home=private)
        self.assertEqual([row.identifier for row in markers], ["host-home-directory", "netrc-credentials-file"])
        self.assertEqual(markers[0].needle, private)
        with self.assertRaises(self.mod.RefusedError):
            self.mod.frozen_markers(rows, private_host_home=b"/Users/other")
        with self.assertRaises(self.mod.RefusedError):
            self.mod.frozen_markers(rows, private_host_home=None)

    def test_any_host_identity_raw_hit_refuses_before_a_pass_record(self) -> None:
        clean = (
            self.mod.RawOccurrence("netrc-credentials-file", "secret-shape", 123),
        )
        self.mod.require_no_host_identity_hits(clean)
        contaminated = clean + (
            self.mod.RawOccurrence("host-home-directory", "host-identity", 456),
        )
        with self.assertRaises(self.mod.RefusedError):
            self.mod.require_no_host_identity_hits(contaminated)

    def test_producer_build_home_marker_comes_from_the_sealed_preflight(self) -> None:
        preflight = {
            "provenance": {
                "repositoryRoot": "/home/runner/work/Boole/Boole",
                "artifactStore": "/home/runner/work/Boole/Boole/local-docs/cas",
            }
        }
        marker = self.mod.producer_build_home_marker(preflight)
        self.assertEqual(marker.identifier, "producer-build-home-directory")
        self.assertEqual(marker.tier, "producer-build-provenance")
        self.assertEqual(marker.needle, b"/home/runner")
        for changed in (
            {},
            {"provenance": {"repositoryRoot": "/tmp/repo", "artifactStore": "/tmp/cas"}},
            {
                "provenance": {
                    "repositoryRoot": "/home/runner/work/Boole/Boole",
                    "artifactStore": "/other/cas",
                }
            },
        ):
            with self.subTest(changed=changed), self.assertRaises(self.mod.RefusedError):
                self.mod.producer_build_home_marker(changed)

    def test_incomplete_or_duplicate_slt_fields_are_refused(self) -> None:
        incomplete = """
Path = etc/passwd
Size = 9
Mode = -r--r--r--
User = 0
"""
        duplicate = """
Path = etc/passwd
Path = etc/shadow
Size = 9
Mode = -r--r--r--
User = 0
Group = 0
"""
        negative = """
Path = etc/passwd
Size = -1
Mode = -r--r--r--
User = 0
Group = 0
"""
        for text in (incomplete, duplicate, negative):
            with self.subTest(text=text), self.assertRaises(self.mod.RefusedError):
                self.mod.parse_slt(text)

    def test_forbidden_logical_paths_are_checked_as_paths_not_substrings(self) -> None:
        self.assertEqual(
            self.mod.forbidden_logical_path_hits(
                ["usr/share/doc/netrc.txt", "home/user/.netrc", ".boole/keys/key.json"]
            ),
            [".boole/keys/key.json", "home/user/.netrc"],
        )
        self.assertEqual(
            self.mod.forbidden_logical_path_hits(["usr/lib/libboole-artifacts.so"]),
            [],
        )

    def test_forbidden_logical_path_scan_materializes_a_one_shot_iterator(self) -> None:
        paths = (
            path
            for path in (
                "usr/share/doc/netrc.txt",
                "home/user/.netrc",
                ".boole/keys/key.json",
            )
        )
        self.assertEqual(
            self.mod.forbidden_logical_path_hits(paths),
            [".boole/keys/key.json", "home/user/.netrc"],
        )


class GuestSecretPathContentReconcileResultTests(unittest.TestCase):
    RESULT = (
        REPO
        / "native/containment/native-shadow-mac3-guest-secret-path-content-reconciliation-arm64-v1.json"
    )

    def setUp(self) -> None:
        self.raw = self.RESULT.read_bytes()
        self.result = json.loads(self.raw.decode("utf-8"))

    def test_result_is_canonical_and_records_only_the_one_condition(self) -> None:
        self.assertEqual(self.raw, module().canonical_json(self.result))
        self.assertEqual(
            self.result["schema"],
            "boole.native-shadow.mac3.guest-secret-path-content-reconciliation.arm64.v1",
        )
        self.assertEqual(
            self.result["status"],
            (
                "LOGICAL-PATH-CONTENT-AND-PHYSICAL-OWNER-RECONCILIATION-PASS-"
                "HOST-PATH-CONDITION-NOT-SETTLED"
            ),
        )
        self.assertTrue(self.result["reconciliationPassed"])
        self.assertFalse(self.result["conditionSettled"])
        self.assertEqual(self.result["verdict"], "NOT-SETTLED")
        self.assertEqual(
            self.result["condition"],
            "no-host-wallet-model-key-or-node-secret-in-the-guest",
        )

    def test_target_is_replica_one_and_was_byte_unchanged(self) -> None:
        target = self.result["targetImage"]
        self.assertEqual(target["archiveRelativePath"], "successor-outputs-1/guest-root-disk")
        self.assertEqual(target["replica"], 1)
        self.assertEqual(target["sizeBytes"], 2_035_625_984)
        self.assertEqual(
            target["sha256Before"],
            "51410d8113c28d6cd28c7b6c7578076226d5e19b6629649199af7b7f86540a1c",
        )
        self.assertEqual(target["sha256After"], target["sha256Before"])
        self.assertFalse(target["archiveRelativePath"].startswith("/"))

    def test_expected_and_observed_logical_trees_are_exact(self) -> None:
        expected = self.result["expectedTree"]
        self.assertEqual(expected["entries"], 17_677)
        self.assertEqual(
            expected["byKind"],
            {"directory": 1_738, "file": 15_102, "symlink": 837},
        )
        self.assertEqual(expected["pathManifestBytes"], 970_123)
        self.assertEqual(
            expected["pathManifestSha256"],
            "a6a7d0e858e62ca2f686b1d13ade4e10e9e97a49bbea582a2a76217236923fb6",
        )
        logical = self.result["logicalTree"]
        self.assertEqual(logical["logicalPaths"], expected["entries"])
        self.assertEqual(logical["byKind"], expected["byKind"])
        self.assertEqual(logical["regularFilesHashed"], 15_102)
        self.assertEqual(logical["symlinkTargetsChecked"], 837)
        self.assertEqual(logical["forbiddenLogicalPaths"], 0)
        self.assertEqual(logical["forbiddenSymlinkTargets"], 0)
        self.assertEqual(logical["journalBytesScanned"], 33_554_432)
        self.assertEqual(logical["journalHits"], 0)

    def test_every_allocated_ext4_object_is_checksum_bound_and_owned(self) -> None:
        ext4 = self.result["ext4"]
        self.assertEqual(ext4["blockBytes"], 4_096)
        self.assertEqual(ext4["blocks"], 496_979)
        self.assertEqual(ext4["groups"], 16)
        self.assertEqual(ext4["groupDescriptorsVerified"], 16)
        self.assertEqual(ext4["allocatedBlocks"], 435_530)
        self.assertEqual(ext4["allocatedInodes"], 17_687)
        self.assertEqual(ext4["externalXattrBlocks"], 0)
        self.assertEqual(ext4["inlineXattrInodes"], 0)
        self.assertEqual(sum(ext4["physicalOwnerCounts"].values()), 435_530)
        self.assertEqual(
            ext4["physicalOwnerCounts"],
            {
                "allocation-metadata": 1_440,
                "directory-data": 1_766,
                "extent-metadata": 24,
                "file-data": 422_636,
                "journal": 8_192,
                "resize-metadata": 1_453,
                "super-gdt": 12,
                "symlink-data": 7,
            },
        )

    def test_all_135_hits_are_conserved_without_metadata_or_journal_hits(self) -> None:
        raw = self.result["rawReconciliation"]
        self.assertEqual(raw["markersSearched"], 25)
        self.assertEqual(raw["rawHits"], 135)
        self.assertEqual(raw["logicalFileHits"], 135)
        self.assertEqual(raw["attributedRawHits"], 135)
        self.assertEqual(sum(raw["markerCounts"].values()), 135)
        for key in (
            "hostIdentityHits",
            "journalHits",
            "directoryHits",
            "symlinkHits",
            "metadataHits",
            "slackHits",
            "freeBlockHits",
            "unmappedHits",
            "ambiguousHits",
        ):
            self.assertEqual(raw[key], 0, key)

    def test_producer_build_path_is_attributed_only_to_the_sealed_launcher(self) -> None:
        observation = self.result["producerBuildPathObservation"]
        self.assertEqual(observation["marker"], "producer-build-home-directory")
        self.assertEqual(observation["needleBytes"], 12)
        self.assertEqual(len(observation["needleSha256"]), 64)
        self.assertEqual(observation["rawHits"], 23)
        self.assertEqual(observation["attributedRawHits"], 23)
        self.assertEqual(observation["distinctOwnerInodes"], 1)
        self.assertEqual(
            observation["paths"],
            ["usr/libexec/boole/boole-native-shadow-launcher"],
        )
        self.assertEqual(
            observation["fileSha256"],
            "11b5d1cf1728aff271c589129292bcd8ad07a1d928652d2435b1c9010f73c434",
        )
        self.assertEqual(
            observation["classification"],
            (
                "SEALED-LAUNCHER-BUILD-PROVENANCE-PATH-NOT-SECRET-MATERIAL-"
                "BUT-HOST-PATH-CONDITION-BLOCKER"
            ),
        )
        self.assertFalse(observation["zeroHostDerivedBuildPathStringsClaim"])
        self.assertFalse(observation["hostPathCriterionMet"])
        self.assertTrue(observation["noHostWalletModelOrNodeSecretMaterialObserved"])
        self.assertNotIn("needle", observation)

    def test_mapping_rows_are_full_span_unique_and_reveal_no_candidate_bytes(self) -> None:
        rows = self.result["rawReconciliation"]["mappings"]
        self.assertEqual(len(rows), 135)
        identities = []
        for row in rows:
            identities.append((row["rawOffset"], row["marker"]))
            self.assertGreater(row["needleBytes"], 0)
            self.assertEqual(row["rawEnd"], row["rawOffset"] + row["needleBytes"])
            self.assertGreater(row["inode"], 0)
            self.assertGreaterEqual(row["fileOffset"], 0)
            self.assertTrue(row["paths"])
            self.assertEqual(len(row["fileSha256"]), 64)
            self.assertEqual(len(row["physicalBlocks"]), len(row["blockSha256"]))
            self.assertTrue(row["physicalBlocks"])
            self.assertFalse({"needle", "context", "surroundingBytes"} & set(row))
        self.assertEqual(identities, sorted(set(identities)))

    def test_authority_bindings_are_exactly_the_checked_in_seals(self) -> None:
        observed = {row["path"]: row["sha256"] for row in self.result["authorityBindings"]}
        self.assertEqual(observed, module().AUTHORITY_SHA256)
        self.assertEqual(
            observed[
                "native/containment/native-shadow-mac3-successor-preflight-result-arm64-v1.json"
            ],
            "be4a84e1c058fa25804cfade07727e35613369f58b0307182b93f24a4ecfb071",
        )
        for path, digest in observed.items():
            self.assertEqual(hashlib.sha256((REPO / path).read_bytes()).hexdigest(), digest)

    def test_reader_and_runtime_implementations_are_bound_without_host_paths(self) -> None:
        bindings = self.result["implementationBindings"]
        expected_scripts = {
            "scripts/native_shadow_ext4_readonly_owner_map_arm64_v1.py",
            "scripts/native_shadow_mac3_guest_secret_path_content_reconcile_arm64_v1.py",
        }
        observed_scripts = {row["path"] for row in bindings["scripts"]}
        self.assertEqual(observed_scripts, expected_scripts)
        for row in bindings["scripts"]:
            raw = (REPO / row["path"]).read_bytes()
            self.assertEqual(row["sizeBytes"], len(raw))
            self.assertEqual(row["sha256"], hashlib.sha256(raw).hexdigest())

        runtime = bindings["pythonRuntime"]
        self.assertEqual(runtime["implementation"], "CPython")
        self.assertRegex(runtime["version"], r"^3\.[0-9]+\.[0-9]+$")
        self.assertEqual(len(runtime["executableSha256"]), 64)
        self.assertGreater(runtime["executableBytes"], 0)

        tools = bindings["sourceAssemblyTools"]
        self.assertEqual({row["name"] for row in tools}, {"gpgv", "zstd"})
        for row in tools:
            self.assertEqual(len(row["sha256"]), 64)
            self.assertGreater(row["sizeBytes"], 0)
            self.assertNotIn("path", row)

        expected_builders = {
            "scripts/native_shadow_boot_staging_measure_arm64_v1.py",
            "scripts/native_shadow_rootfs_builder_boot_arm64_v1.py",
            "scripts/native_shadow_rootfs_builder_boot_arm64_v3.py",
            "scripts/native_shadow_rootfs_portable_boot_arm64_v2.py",
        }
        observed_builders = {row["path"] for row in bindings["expectedTreeBuilders"]}
        self.assertEqual(observed_builders, expected_builders)
        for row in bindings["expectedTreeBuilders"]:
            raw = (REPO / row["path"]).read_bytes()
            self.assertEqual(row["sizeBytes"], len(raw))
            self.assertEqual(row["sha256"], hashlib.sha256(raw).hexdigest())

    def test_record_contains_no_private_marker_or_absolute_host_path(self) -> None:
        text = self.raw.decode("utf-8")
        self.assertNotIn("/Users/", text)
        self.assertNotIn("privateHostHome", text)
        for forbidden in ("sk-ant-", "sk-or-v1-", "sk-proj-"):
            self.assertNotIn(forbidden, text)
        self.assertFalse(self.result["method"]["privateMarkerRecorded"])
        self.assertFalse(self.result["method"]["surroundingBytesRecorded"])

    def test_claim_boundary_remains_closed_local_and_non_activating(self) -> None:
        boundary = self.result["claimBoundary"]
        self.assertEqual(
            boundary,
            {
                "activationAllowed": False,
                "bootAttempted": False,
                "imageProducedOrModified": False,
                "mineableNow": 0,
                "paidApiBenchmarkClaim": False,
                "publicMiningClaim": False,
                "rewardReady": 0,
                "servingClaim": False,
            },
        )


if __name__ == "__main__":
    unittest.main()
