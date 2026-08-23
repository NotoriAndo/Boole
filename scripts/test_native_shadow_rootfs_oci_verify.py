#!/usr/bin/env python3
"""Independent-verifier tests for native-shadow OCI rootfs layouts."""

from __future__ import annotations

import json
import io
import os
import pathlib
import subprocess
import sys
import tarfile
import tempfile
import unittest

from scripts import native_shadow_rootfs_builder as builder
from scripts import native_shadow_rootfs_oci_verify as verifier
from scripts import test_native_shadow_rootfs_builder as builder_tests


class NativeShadowRootfsIndependentVerifierTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        builder_tests.NativeShadowRootfsBuilderTests.setUpClass()

    def setUp(self) -> None:
        self.fixture = builder_tests.NativeShadowRootfsBuilderTests(
            "test_tracked_lock_is_canonical_valid_incomplete_and_build_refuses"
        )

    def _built_layout(
        self, base: pathlib.Path
    ) -> tuple[dict[str, object], bytes, pathlib.Path, pathlib.Path, pathlib.Path, dict[str, object]]:
        lock, raw, repo, store = self.fixture._fixture(base)
        layout = base / "oci"
        receipt = self.fixture._build_fixture(lock, raw, repo, store, layout)
        return lock, raw, repo, store, layout, receipt

    def _verify(
        self,
        layout: pathlib.Path,
        raw: bytes,
        receipt: dict[str, object],
        *,
        source_sha: str | None = None,
        builder_sha: str | None = None,
        layer_digest: str | None = None,
        content_sha: str | None = None,
    ) -> dict[str, object]:
        return verifier.verify_layout(
            layout,
            source_sha or builder_tests._sha(raw),
            builder_sha or builder.BUILDER_SHA256,
            layer_digest or str(receipt["layerDigest"]),
            content_sha or str(receipt["rootfsContentManifestSha256"]),
        )

    def _write_self_consistent_layout(
        self,
        output: pathlib.Path,
        entries: dict[str, dict[str, object]],
        lock: dict[str, object],
        raw: bytes,
    ) -> dict[str, object]:
        content_raw = builder.canonical_json(
            builder._entry_manifest(entries, lock["closureRoots"])
        )
        layer_raw = builder._layer_bytes(entries, 0)
        layer = builder._descriptor(builder.OCI_LAYER_MEDIA_TYPE, layer_raw)
        config_raw = builder.canonical_json(
            {
                "architecture": "amd64",
                "config": {
                    "Env": ["LANG=C", "LC_ALL=C", "TZ=UTC"],
                    "Labels": {"org.boole.native-shadow.activation-allowed": "false"},
                },
                "os": "linux",
                "rootfs": {"diff_ids": [layer["digest"]], "type": "layers"},
            },
            compact=True,
        )
        config = builder._descriptor(builder.OCI_CONFIG_MEDIA_TYPE, config_raw)
        manifest_raw = builder.canonical_json(
            {
                "schemaVersion": 2,
                "mediaType": builder.OCI_MANIFEST_MEDIA_TYPE,
                "config": config,
                "layers": [layer],
                "annotations": {"org.boole.native-shadow.activation-allowed": "false"},
            },
            compact=True,
        )
        manifest = builder._descriptor(builder.OCI_MANIFEST_MEDIA_TYPE, manifest_raw)
        index_raw = builder.canonical_json(
            {
                "schemaVersion": 2,
                "manifests": [
                    {
                        **manifest,
                        "platform": {"architecture": "amd64", "os": "linux"},
                        "annotations": {
                            "org.boole.native-shadow.activation-allowed": "false"
                        },
                    }
                ],
            },
            compact=True,
        )
        receipt = {
            "schema": "boole.native-shadow.runtime-rootfs-build-receipt.v1",
            "authorityStatus": "BUILT-NOT-ACTIVATABLE",
            "activationAllowed": False,
            "productionByteProvenanceComplete": False,
            "sourceLockSha256": builder_tests._sha(raw),
            "builderSha256": builder.BUILDER_SHA256,
            "rootfsContentManifestSha256": builder_tests._sha(content_raw),
            "rootfsContentManifestSizeBytes": len(content_raw),
            "layerDigest": layer["digest"],
            "layerSizeBytes": layer["size"],
            "configDigest": config["digest"],
            "configSizeBytes": config["size"],
            "manifestDigest": manifest["digest"],
            "manifestSizeBytes": manifest["size"],
            "indexSha256": builder_tests._sha(index_raw),
            "indexSizeBytes": len(index_raw),
            "layerCount": 1,
            "parentLayerCount": 0,
        }
        (output / "blobs" / "sha256").mkdir(parents=True)
        for directory in (output, output / "blobs", output / "blobs" / "sha256"):
            directory.chmod(0o755)
        builder._write_blob(output, layer, layer_raw)
        builder._write_blob(output, config, config_raw)
        builder._write_blob(output, manifest, manifest_raw)
        sidecars = {
            "oci-layout": builder.canonical_json(
                {"imageLayoutVersion": "1.0.0"}, compact=True
            ),
            "index.json": index_raw,
            "ROOTFS-CONTENT-MANIFEST.json": content_raw,
            "BUILD-RECEIPT.json": builder.canonical_json(receipt),
        }
        for name, payload in sidecars.items():
            path = output / name
            path.write_bytes(payload)
            path.chmod(0o444)
        return receipt

    def test_verifier_is_independent_and_accepts_real_builder_output(self) -> None:
        source = pathlib.Path(verifier.__file__).read_text(encoding="utf-8")
        self.assertNotIn("native_shadow_rootfs_builder", source)
        with tempfile.TemporaryDirectory() as tmp:
            _, raw, _, _, layout, receipt = self._built_layout(pathlib.Path(tmp))
            self.assertEqual(self._verify(layout, raw, receipt), receipt)

    def test_external_source_and_builder_identities_are_mandatory(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            _, raw, _, _, layout, receipt = self._built_layout(pathlib.Path(tmp))
            for source_sha, builder_sha, layer_digest, content_sha in (
                ("00" * 32, builder.BUILDER_SHA256, None, None),
                (None, "11" * 32, None, None),
                (None, None, "sha256:" + ("22" * 32), None),
                (None, None, None, "33" * 32),
            ):
                with self.subTest(
                    source_sha=source_sha,
                    builder_sha=builder_sha,
                    layer_digest=layer_digest,
                    content_sha=content_sha,
                ):
                    with self.assertRaises(verifier.OciVerificationError):
                        self._verify(
                            layout,
                            raw,
                            receipt,
                            source_sha=source_sha,
                            builder_sha=builder_sha,
                            layer_digest=layer_digest,
                            content_sha=content_sha,
                        )

    def test_self_consistent_receipt_cannot_replace_externally_pinned_layer(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            lock, raw, repo, store, _, original = self._built_layout(base)
            validated = self.fixture._validate_fixture(lock, raw, repo, store)
            entries = builder._assemble_entries(validated, repo, store)
            authority_paths = {
                item["logicalPath"].lstrip("/") for item in lock["trackedFiles"]
            }
            forged_entries = {
                path: entry for path, entry in entries.items() if path not in authority_paths
            }
            forged = self._write_self_consistent_layout(
                base / "forged", forged_entries, lock, raw
            )
            self.assertEqual(forged["sourceLockSha256"], original["sourceLockSha256"])
            self.assertNotEqual(forged["layerDigest"], original["layerDigest"])
            with self.assertRaisesRegex(
                verifier.OciVerificationError, "external authority"
            ):
                self._verify(base / "forged", raw, original)

    def test_symlink_extra_file_and_receipt_tamper_are_rejected(self) -> None:
        for scenario in ("symlink", "extra", "receipt"):
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as tmp:
                base = pathlib.Path(tmp)
                _, raw, _, _, layout, receipt = self._built_layout(base)
                if scenario == "symlink":
                    index = layout / "index.json"
                    outside = base / "outside-index.json"
                    outside.write_bytes(index.read_bytes())
                    index.unlink()
                    index.symlink_to(outside)
                elif scenario == "extra":
                    extra = layout / "unexpected"
                    extra.write_bytes(b"extra")
                    extra.chmod(0o444)
                else:
                    receipt_path = layout / "BUILD-RECEIPT.json"
                    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
                    receipt["sourceLockSha256"] = "22" * 32
                    receipt_path.chmod(0o644)
                    receipt_path.write_bytes(builder.canonical_json(receipt))
                    receipt_path.chmod(0o444)
                with self.assertRaises(verifier.OciVerificationError):
                    self._verify(layout, raw, receipt)

    def test_cli_rejects_without_traceback(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            _, raw, _, _, layout, receipt = self._built_layout(pathlib.Path(tmp))
            completed = subprocess.run(
                [
                    sys.executable,
                    str(pathlib.Path(verifier.__file__).resolve()),
                    "verify",
                    "--layout",
                    str(layout),
                    "--expected-source-lock-sha256",
                    "00" * 32,
                    "--expected-builder-sha256",
                    builder.BUILDER_SHA256,
                    "--expected-layer-digest",
                    str(receipt["layerDigest"]),
                    "--expected-content-manifest-sha256",
                    str(receipt["rootfsContentManifestSha256"]),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(completed.returncode, 2)
            self.assertIn("native-shadow-rootfs-oci-verify: FAIL", completed.stderr)
            self.assertNotIn("Traceback", completed.stderr)
            self.assertEqual(completed.stdout, "")

    def test_layer_requires_explicit_parent_directories(self) -> None:
        raw = builder._layer_bytes(
            {
                "usr/bin/python3.12": {
                    "kind": "file",
                    "mode": 0o555,
                    "raw": b"python",
                }
            },
            0,
        )
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            layer_path = base / "layer"
            layer_path.write_bytes(raw)
            layer_path.chmod(0o444)
            parent = os.open(base, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
            opened = verifier._open_file_at(parent, "layer", "test layer")
            try:
                with self.assertRaisesRegex(
                    verifier.OciVerificationError, "parent directory"
                ):
                    verifier._layer_entries(opened)
            finally:
                opened.close()
                os.close(parent)

    def test_layer_rejects_oversized_pax_header_before_tarfile_parsing(self) -> None:
        raw = self.fixture._tar(
            [
                {
                    "name": "x",
                    "raw": b"x",
                    "mtime": 0,
                    "pax": {"path": "x" * (verifier.MAX_PAX_HEADER_BYTES + 1)},
                }
            ]
        )
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            layer_path = base / "layer"
            layer_path.write_bytes(raw)
            layer_path.chmod(0o444)
            parent = os.open(base, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
            opened = verifier._open_file_at(parent, "layer", "test layer")
            try:
                with self.assertRaisesRegex(
                    verifier.OciVerificationError, "PAX header exceeds"
                ):
                    verifier._layer_entries(opened)
            finally:
                opened.close()
                os.close(parent)

    def test_deep_json_is_a_typed_reject_not_a_traceback(self) -> None:
        depth = verifier.MAX_JSON_NESTING + 1
        hostile = (b"[" * depth) + b"0" + (b"]" * depth) + b"\n"
        with self.assertRaisesRegex(verifier.OciVerificationError, "JSON nesting"):
            verifier._json_document(hostile, "hostile sidecar", compact=True)

    def test_raw_pax_header_metadata_must_match_builder_envelope(self) -> None:
        raw = bytearray(
            builder._layer_bytes(
                {"x" * 120: {"kind": "file", "mode": 0o444, "raw": b"x"}}, 0
            )
        )
        self.assertEqual(raw[156:157], b"x")
        raw[108:116] = b"0000001\0"
        raw[148:156] = b"        "
        checksum = sum(raw[:512])
        raw[148:156] = f"{checksum:06o}\0 ".encode("ascii")
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            path = base / "layer"
            path.write_bytes(raw)
            path.chmod(0o444)
            parent = os.open(base, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
            opened = verifier._open_file_at(parent, "layer", "test layer")
            try:
                with self.assertRaisesRegex(
                    verifier.OciVerificationError, "PAX header metadata"
                ):
                    verifier._layer_entries(opened)
            finally:
                opened.close()
                os.close(parent)

    def test_builder_impossible_hardlink_layer_is_rejected(self) -> None:
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode="w", format=tarfile.PAX_FORMAT) as archive:
            regular = tarfile.TarInfo("a")
            regular.mode = 0o444
            regular.size = len(b"payload")
            regular.mtime = 0
            archive.addfile(regular, io.BytesIO(b"payload"))
            hardlink = tarfile.TarInfo("z")
            hardlink.type = tarfile.LNKTYPE
            hardlink.mode = 0o444
            hardlink.linkname = "a"
            hardlink.mtime = 0
            archive.addfile(hardlink)
        raw = stream.getvalue()
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            path = base / "layer"
            path.write_bytes(raw)
            path.chmod(0o444)
            parent = os.open(base, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
            opened = verifier._open_file_at(parent, "layer", "test layer")
            try:
                with self.assertRaisesRegex(verifier.OciVerificationError, "hardlink"):
                    verifier._layer_entries(opened)
            finally:
                opened.close()
                os.close(parent)


if __name__ == "__main__":
    unittest.main()
