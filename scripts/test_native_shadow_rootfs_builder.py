#!/usr/bin/env python3
"""Contract tests for the native-shadow offline deterministic rootfs builder."""

from __future__ import annotations

import base64
import hashlib
import io
import json
import lzma
import os
import pathlib
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import unittest

from scripts import native_shadow_provenance_manifest as provenance
from scripts import native_shadow_rootfs_builder as rootfs


ROOT = pathlib.Path(__file__).resolve().parents[1]
TRACKED_LOCK = ROOT / "native/containment/native-shadow-runtime-rootfs-source-lock-v1.json"
TEST_SIGNER_FINGERPRINT = "426211D6DE8FB032718493CF3A71BFD8616C17EE"
TEST_KEYRING = base64.b64decode(
    """
mQENBGqK1tABCACfEfDrf63BVDGe3QnKMoaHVX5Ozwb28pP9S0gzOitUN2Kfo+4p/l4kKDTSfwOXKAehDjoBxM7J
abSiiMem5CWoApyapJmoxth1sm3rBNcJbJv5f8oW8NypJpLnXi76CbHTyym1TwBjqCoinGEypEYMHkzHStj15I6+
AzbXryfySG5TInyoDIKBa6LIEHO7zHRIQDtZh7zUbmlWRYegDMRr223GXQvEZAHdYV8SegWOHXhAx0/8c81eJKZ+
lpF8H5GAC+2/fb3l2UF+4tO1NhmcZh0lFoTmYD6zT8q/87m0P4GANaAk8BAjhWiYwYlwjoQVtaqWFVIVujejL3Z8
H7dFABEBAAG0L0Jvb2xlIFJvb3RmcyBUZXN0IDxyb290ZnMtdGVzdEBpbnZhbGlkLmV4YW1wbGU+iQFRBBMBCAA7
FiEEQmIR1t6PsDJxhJPPOnG/2GFsF+4FAmqK1tACGwMFCwkIBwICIgIGFQoJCAsCBBYCAwECHgcCF4AACgkQOnG/
2GFsF+4F4gf/TM4tRJ1tsRo6iPgrbZwojDNDyB3Jgo21VyPnnkxBj1L69DSRuMKtQG5G08vunjoBxtklEeXhCp28
lAN9cS1TV0fM7yahzPp5S/Efg526NJcP8LHqN934ilGFItosWb3EkOUKY3PfTHB7RAI4V6SZYJDVG3gA5HHD72bN
WaSofqj7EK48ex0+UyiNDjq1dUqre+UHgxf5xOSjK5YAzm6IhFNEF7KPnyF8m6M09YbtG8aU4gFeqsj5x4sjyExR
L6e8ycDW9g9H0uIPWrXJQXJ3sAAYGjea7jcFJyLPie693nAMkkKBsCeWV/9gibTC5NoUFYti6sYoV2VaKE0LSysW
Jw==
"""
)
TEST_INRELEASE = base64.b64decode(
    """
LS0tLS1CRUdJTiBQR1AgU0lHTkVEIE1FU1NBR0UtLS0tLQpIYXNoOiBTSEEyNTYKClN1aXRlOiBub2JsZQpDb2Rl
bmFtZTogbm9ibGUKRGF0ZTogU2F0LCAyMiBBdWcgMjAyNiAwMDowMDowMCArMDAwMApWYWxpZC1VbnRpbDogTW9u
LCAyNCBBdWcgMjAyNiAwMDowMDowMCArMDAwMApBcmNoaXRlY3R1cmVzOiBhbWQ2NApDb21wb25lbnRzOiBtYWlu
ClNIQTI1NjoKIGVmZjBhNTc2OGUwYjZjYzljMTA0N2JlZDRiNTM4MjZlODI2MTFiZWI2NTA2ZWU4OGMyOTBhMDdk
ZGE5NmU5ZWYgOTYzIG1haW4vYmluYXJ5LWFtZDY0L1BhY2thZ2VzCi0tLS0tQkVHSU4gUEdQIFNJR05BVFVSRS0t
LS0tCgppUUV6QkFFQkNBQWRGaUVFUW1JUjF0NlBzREp4aEpQUE9uRy8yR0ZzRis0RkFtcUsxdEVBQ2drUU9uRy8y
R0ZzCkYrN2FwUWYrSllPMW4zMWdzZ0RSdmdvVnRndUk4dHJWektyS1JjYW1hNXNZekxXcGRaYU1PN0crRlh1anVa
L0cKZXJWWVpkVmdwUGhjamJzb1ZJS2JCTUowVitTSGd2Z21sRFhpbnp4OFZsQndjU3NHbHRweHBWUU1PMWVqTHI1
TgplUkY5TWhpYmNZMVh2R0o2RmMweGJtUkhyMVl4L3puRE5rQ2txdU5NZTkzdjJTcXNmSTFSbEhZYk9EOFJOZUJu
CmhMLy9BNHloS2UrQjVHb0Q3ZVFhVndHQTQ3aDZnVnhRRS93aU9KbDRFVnRPMVgySUIrVHlMeldycTdNNUI5TE4K
d011Tk1NSGhNbUFRUXFqQ1d1bXFuMGZuNWFTcFUwRm5aU0R6eXZ0S3hYa0Fzak5LK1BDRU0xbEQxT1FRMmNZZQp3
OEdWOEpXQThVUTBLb3gzNk9vQXc0RUdueFNhcnc9PQo9NmZFUwotLS0tLUVORCBQR1AgU0lHTkFUVVJFLS0tLS0K
"""
)


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


class NativeShadowRootfsBuilderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        gpgv = shutil.which("gpgv")
        if gpgv is None:
            raise RuntimeError("rootfs supply-chain tests require gpgv")
        cls._gpgv = pathlib.Path(os.path.realpath(gpgv))

    @property
    def _trusted_fingerprints(self) -> frozenset[str]:
        return frozenset({TEST_SIGNER_FINGERPRINT})
    def _tar(self, entries: list[dict[str, object]], *, compressed: bool = False) -> bytes:
        stream = io.BytesIO()
        archive_format = (
            tarfile.PAX_FORMAT if any("pax" in entry for entry in entries) else tarfile.GNU_FORMAT
        )
        with tarfile.open(fileobj=stream, mode="w", format=archive_format) as archive:
            for entry in entries:
                info = tarfile.TarInfo(str(entry["name"]))
                info.mode = int(entry.get("mode", 0o555))
                info.uid = int(entry.get("uid", 0))
                info.gid = int(entry.get("gid", 0))
                info.mtime = int(entry.get("mtime", 123456789))
                kind = entry.get("kind", "file")
                if kind == "directory":
                    info.type = tarfile.DIRTYPE
                    archive.addfile(info)
                elif kind == "symlink":
                    info.type = tarfile.SYMTYPE
                    info.linkname = str(entry["target"])
                    archive.addfile(info)
                elif kind == "hardlink":
                    info.type = tarfile.LNKTYPE
                    info.linkname = str(entry["target"])
                    archive.addfile(info)
                elif kind == "fifo":
                    info.type = tarfile.FIFOTYPE
                    archive.addfile(info)
                else:
                    raw = bytes(entry.get("raw", b""))
                    info.type = tarfile.REGTYPE
                    info.size = len(raw)
                    if "pax" in entry:
                        info.pax_headers = dict(entry["pax"])
                    archive.addfile(info, io.BytesIO(raw))
        raw = stream.getvalue()
        return lzma.compress(raw) if compressed else raw

    def _ar(self, members: list[tuple[str, bytes]]) -> bytes:
        output = bytearray(b"!<arch>\n")
        for name, raw in members:
            header = (
                f"{name + '/':<16}{0:<12}{0:<6}{0:<6}{0:<8}{len(raw):<10}`\n"
            ).encode("ascii")
            self.assertEqual(len(header), 60)
            output.extend(header)
            output.extend(raw)
            if len(raw) % 2:
                output.extend(b"\n")
        return bytes(output)

    def _deb(self, entries: list[dict[str, object]], *, postinst: bytes = b"SENTINEL-MUST-NOT-RUN") -> bytes:
        control = self._tar(
            [
                {"name": "control", "raw": b"Package: synthetic\n"},
                {"name": "postinst", "raw": postinst, "mode": 0o755},
            ],
            compressed=True,
        )
        data = self._tar(entries, compressed=True)
        return self._ar(
            [
                ("debian-binary", b"2.0\n"),
                ("control.tar.xz", control),
                ("data.tar.xz", data),
            ]
        )

    def _rust_dist(self, top: str, component: str, files: list[dict[str, object]]) -> bytes:
        manifest = "".join(f"file:{item['name']}\n" for item in files).encode("utf-8")
        entries: list[dict[str, object]] = [
            {"name": f"{top}/components", "raw": (component + "\n").encode("utf-8")},
            {"name": f"{top}/{component}/manifest.in", "raw": manifest},
            {
                "name": f"{top}/{component}/install.sh",
                "raw": b"RUST-INSTALL-SCRIPT-MUST-NOT-COPY-OR-RUN",
                "mode": 0o755,
            },
        ]
        for item in files:
            copied = dict(item)
            copied["name"] = f"{top}/{component}/{item['name']}"
            entries.append(copied)
        return self._tar(entries, compressed=True)

    def _write_repo(self, root: pathlib.Path, rust_hashes: dict[str, str]) -> dict[str, bytes]:
        files = {
            "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json": rootfs.canonical_json(
                {
                    "activationAllowed": False,
                    "qualificationFixture": {"nonIssuable": True},
                    "release": "synthetic",
                }
            ),
            "native/checker/rust-tuple-struct-project-v1/checker.py": b"#!/usr/bin/python3.12\nprint('checker')\n",
            "native/checker/rust-tuple-struct-project-v1/policy.json": rootfs.canonical_json(
                {"activationAllowed": False}
            ),
            "native/containment/native-shadow-execution-policy-v1.json": rootfs.canonical_json(
                {
                    "activationAllowed": False,
                    "checkerInvocation": {"executionAllowedUnderThisRelease": False},
                }
            ),
            "fixtures/native-shadow/registry-v1.json": rootfs.canonical_json(
                {"activationAllowed": False, "templates": [{"nonIssuable": True}]}
            ),
        }
        identity = {
            "schema": "boole.native-shadow.toolchain-identity.v1",
            "activationAllowed": False,
            "rust": {
                "rustcCommitHash": "11" * 20,
                "cargoCommitHash": "22" * 20,
                "linuxX8664ArtifactSha256": rust_hashes,
            },
            "runtimeVerification": {
                "productionByteProvenanceComplete": False,
                "executionAllowedBeforeProvenanceClosure": False,
            },
        }
        files["native/containment/native-shadow-toolchain-identity-v1.json"] = rootfs.canonical_json(
            identity
        )
        for relative, raw in files.items():
            path = root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)
        return files

    def _store(self, root: pathlib.Path, raw: bytes) -> tuple[str, int]:
        digest = _sha(raw)
        path = root / "sha256" / digest
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(raw)
        return digest, len(raw)

    def _fixture(self, base: pathlib.Path) -> tuple[dict[str, object], bytes, pathlib.Path, pathlib.Path]:
        store = base / "store"
        repo = base / "repo"
        repo.mkdir()
        rust_sources = {
            "cargo": self._rust_dist(
                "cargo-nightly-x86_64-unknown-linux-gnu",
                "cargo",
                [{"name": "bin/cargo", "raw": b"cargo-runtime", "mode": 0o555}],
            ),
            "rust-std-x86_64-unknown-linux-gnu": self._rust_dist(
                "rust-std-nightly-x86_64-unknown-linux-gnu",
                "rust-std-x86_64-unknown-linux-gnu",
                [
                    {
                        "name": "lib/rustlib/x86_64-unknown-linux-gnu/lib/libstd.rlib",
                        "raw": b"rust-std-runtime",
                        "mode": 0o444,
                    }
                ],
            ),
            "rustc": self._rust_dist(
                "rustc-nightly-x86_64-unknown-linux-gnu",
                "rustc",
                [
                    {"name": "bin/rustc", "raw": b"rustc-runtime", "mode": 0o555},
                    {"name": "lib/libLLVM.so", "raw": b"llvm-runtime", "mode": 0o444},
                ],
            ),
        }
        rust_filenames = {
            "cargo": "cargo-nightly-x86_64-unknown-linux-gnu.tar.xz",
            "rust-std-x86_64-unknown-linux-gnu": "rust-std-nightly-x86_64-unknown-linux-gnu.tar.xz",
            "rustc": "rustc-nightly-x86_64-unknown-linux-gnu.tar.xz",
        }
        rust_hashes = {
            rust_filenames[name]: _sha(raw) for name, raw in rust_sources.items()
        }
        repo_files = self._write_repo(repo, rust_hashes)

        artifact_values: dict[str, tuple[str, str, int]] = {}
        for name, raw in rust_sources.items():
            digest, size = self._store(store, raw)
            artifact_values[f"{name}-rustdist"] = ("rust-dist", digest, size)

        debs = {
            "binutils-x86-64-linux-gnu": self._deb(
                [{"name": "./usr/bin/ld", "raw": b"ld-runtime", "mode": 0o555}]
            ),
            "gcc-13-x86-64-linux-gnu": self._deb(
                [
                    {
                        "name": "./usr/bin/x86_64-linux-gnu-gcc-13",
                        "raw": b"gcc-runtime",
                        "mode": 0o555,
                    }
                ]
            ),
            "libc6-dev": self._deb(
                [
                    {
                        "name": "./lib/x86_64-linux-gnu/ld-linux-x86-64.so.2",
                        "raw": b"loader-runtime",
                        "mode": 0o555,
                    },
                    {
                        "name": "./lib/x86_64-linux-gnu/libc.so.6",
                        "raw": b"libc-runtime",
                        "mode": 0o444,
                    },
                    {
                        "name": "./usr/lib/x86_64-linux-gnu/crt1.o",
                        "raw": b"crt-runtime",
                        "mode": 0o444,
                    },
                ]
            ),
            "python3.12": self._deb(
                [
                    {"name": "./usr/bin/python3.12", "raw": b"python-runtime", "mode": 0o555},
                    {"name": "./usr/lib/python3.12/os.py", "raw": b"# stdlib\n", "mode": 0o444},
                ]
            ),
        }
        for name, raw in debs.items():
            digest, size = self._store(store, raw)
            artifact_values[f"deb-{name}"] = ("deb", digest, size)

        dependency_fields = {
            "binutils-x86-64-linux-gnu": ("", ""),
            "gcc-13-x86-64-linux-gnu": (
                "binutils-x86-64-linux-gnu (>= 1.0-1)",
                "",
            ),
            "libc6-dev": ("", ""),
            "python3.12": ("", "libc6-dev (= 1.0-1)"),
        }
        stanza_by_name: dict[str, bytes] = {}
        for name in sorted(debs):
            _, digest, size = artifact_values[f"deb-{name}"]
            depends, pre_depends = dependency_fields[name]
            lines = [
                f"Package: {name}",
                "Version: 1.0-1",
                "Architecture: amd64",
                f"Source: {name} (1.0-1)",
                f"Filename: pool/main/{name}.deb",
                f"Size: {size}",
                f"SHA256: {digest}",
            ]
            if depends:
                lines.append(f"Depends: {depends}")
            if pre_depends:
                lines.append(f"Pre-Depends: {pre_depends}")
            stanza_by_name[name] = ("\n".join(lines) + "\n").encode("utf-8")
        packages_index = b"\n".join(stanza_by_name[name] for name in sorted(stanza_by_name))
        packages_path = "main/binary-amd64/Packages"
        release = (
            "Suite: noble\n"
            "Codename: noble\n"
            "Date: Sat, 22 Aug 2026 00:00:00 +0000\n"
            "Valid-Until: Mon, 24 Aug 2026 00:00:00 +0000\n"
            "Architectures: amd64\n"
            "Components: main\n"
            "SHA256:\n"
            f" {_sha(packages_index)} {len(packages_index)} {packages_path}\n"
        ).encode("utf-8")
        self.assertIn(release, TEST_INRELEASE)
        fixed = {
            "ubuntu-keyring": ("ubuntu-keyring", TEST_KEYRING),
            "ubuntu-inrelease": ("ubuntu-inrelease", TEST_INRELEASE),
            "ubuntu-packages-index": ("ubuntu-packages-index", packages_index),
        }
        for identifier, (kind, raw) in fixed.items():
            digest, size = self._store(store, raw)
            artifact_values[identifier] = (kind, digest, size)

        artifacts = [
            {"id": identifier, "kind": kind, "sizeBytes": size, "sha256": digest}
            for identifier, (kind, digest, size) in sorted(artifact_values.items())
        ]
        bindings = [
            {"id": identifier, "sourcePath": source, "sha256": _sha(repo_files[source])}
            for identifier, source in sorted(
                {
                    "checker-entrypoint": "native/checker/rust-tuple-struct-project-v1/checker.py",
                    "checker-policy": "native/checker/rust-tuple-struct-project-v1/policy.json",
                    "checker-release": "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json",
                    "execution-policy": "native/containment/native-shadow-execution-policy-v1.json",
                    "registry": "fixtures/native-shadow/registry-v1.json",
                    "toolchain-identity": "native/containment/native-shadow-toolchain-identity-v1.json",
                }.items()
            )
        ]
        logical_paths = {
            "native/checker/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json": "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/RELEASE-MANIFEST.json",
            "native/checker/rust-tuple-struct-project-v1/checker.py": "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py",
            "native/checker/rust-tuple-struct-project-v1/policy.json": "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/policy.json",
            "native/containment/native-shadow-execution-policy-v1.json": "/usr/share/boole/native-shadow/execution-policy-v1.json",
            "fixtures/native-shadow/registry-v1.json": "/usr/share/boole/native-shadow/registry-v1.json",
            "native/containment/native-shadow-toolchain-identity-v1.json": "/usr/share/boole/native-shadow/toolchain-identity-v1.json",
        }
        tracked = [
            {
                "sourcePath": source,
                "logicalPath": logical,
                "sha256": _sha(repo_files[source]),
                "mode": "0444",
                "uid": 0,
                "gid": 0,
            }
            for source, logical in sorted(logical_paths.items(), key=lambda item: item[1])
        ]
        packages = [
            {
                "packageId": name,
                "name": name,
                "version": "1.0-1",
                "architecture": "amd64",
                "sourceName": name,
                "sourceVersion": "1.0-1",
                "repositoryId": "noble-main",
                "component": "main",
                "poolPath": f"pool/main/{name}.deb",
                "artifactId": f"deb-{name}",
                "indexStanzaSha256": _sha(stanza_by_name[name]),
                "depends": dependency_fields[name][0],
                "preDepends": dependency_fields[name][1],
                "provides": "",
                "multiArch": "",
                "essential": False,
                "dependencyResolutions": (
                    [
                        {
                            "field": "Depends",
                            "groupIndex": 0,
                            "alternativeIndex": 0,
                            "packageId": "binutils-x86-64-linux-gnu",
                        }
                    ]
                    if name == "gcc-13-x86-64-linux-gnu"
                    else [
                        {
                            "field": "Pre-Depends",
                            "groupIndex": 0,
                            "alternativeIndex": 0,
                            "packageId": "libc6-dev",
                        }
                    ]
                    if name == "python3.12"
                    else []
                ),
            }
            for name in sorted(debs)
        ]
        lock: dict[str, object] = {
            "schema": rootfs.LOCK_SCHEMA,
            "release": "SYNTHETIC-COMPLETE-NOT-ACTIVATABLE",
            "activationAllowed": False,
            "platform": {
                "os": "linux",
                "ociArchitecture": "amd64",
                "debArchitecture": "amd64",
                "rustTarget": "x86_64-unknown-linux-gnu",
            },
            "authorityBindings": bindings,
            "artifacts": artifacts,
            "ubuntu": {
                "snapshot": "2026-08-23T23:00:00Z",
                "verification": {
                    "gpgvPath": str(self._gpgv),
                    "gpgvSha256": _sha(self._gpgv.read_bytes()),
                },
                "repositories": [
                    {
                        "id": "noble-main",
                        "snapshotBase": "synthetic://ubuntu-noble",
                        "suite": "noble",
                        "component": "main",
                        "architecture": "amd64",
                        "keyringArtifactId": "ubuntu-keyring",
                        "inReleaseArtifactId": "ubuntu-inrelease",
                        "packagesIndexArtifactId": "ubuntu-packages-index",
                        "packagesIndexPath": packages_path,
                    }
                ],
                "seeds": ["gcc-13-x86-64-linux-gnu", "python3.12"],
                "seedPackageIds": ["gcc-13-x86-64-linux-gnu", "python3.12"],
                "packages": packages,
            },
            "rust": {
                "rustcCommitHash": "11" * 20,
                "cargoCommitHash": "22" * 20,
                "installPrefix": "/opt/boole/native-checker-toolchain",
                "components": [
                    {
                        "name": name,
                        "target": "x86_64-unknown-linux-gnu",
                        "artifactId": f"{name}-rustdist",
                    }
                    for name in sorted(rust_sources)
                ],
            },
            "trackedFiles": tracked,
            "derivedEntries": [
                {
                    "logicalPath": "/usr/bin/cc",
                    "kind": "symlink",
                    "target": "x86_64-linux-gnu-gcc-13",
                    "mode": "0777",
                    "uid": 0,
                    "gid": 0,
                }
            ],
            "closureRoots": [
                {
                    "name": "installed-rust-toolchain-file-manifest",
                    "logicalRoots": ["/opt/boole/native-checker-toolchain"],
                },
                {
                    "name": "python-interpreter-and-stdlib-file-manifest",
                    "logicalRoots": ["/usr/bin/python3.12", "/usr/lib/python3.12"],
                },
                {
                    "name": "system-linker-and-runtime-file-manifest",
                    "logicalRoots": ["/lib", "/usr/bin", "/usr/lib"],
                },
            ],
            "buildRecipe": {
                "builderSha256": rootfs.BUILDER_SHA256,
                "baseImage": "empty",
                "network": "forbidden",
                "maintainerScripts": "never-execute-or-copy",
                "canonicalMtime": 0,
                "ownership": "root:root-only",
                "output": "oci-image-layout-single-uncompressed-layer-v1",
                "maxEntries": 200000,
                "maxFileBytes": 536870912,
                "maxTotalBytes": 2147483648,
            },
        }
        raw = rootfs.canonical_json(lock)
        return lock, raw, repo, store

    def _replace_package(
        self,
        lock: dict[str, object],
        store: pathlib.Path,
        package_name: str,
        raw: bytes,
    ) -> None:
        digest, size = self._store(store, raw)
        package = next(item for item in lock["ubuntu"]["packages"] if item["name"] == package_name)
        artifact = next(item for item in lock["artifacts"] if item["id"] == package["artifactId"])
        artifact["sha256"] = digest
        artifact["sizeBytes"] = size

    def _replace_artifact(
        self,
        lock: dict[str, object],
        store: pathlib.Path,
        artifact_id: str,
        raw: bytes,
    ) -> None:
        digest, size = self._store(store, raw)
        artifact = next(item for item in lock["artifacts"] if item["id"] == artifact_id)
        artifact["sha256"] = digest
        artifact["sizeBytes"] = size

    def _replace_authority(
        self,
        lock: dict[str, object],
        repo: pathlib.Path,
        source: str,
        raw: bytes,
    ) -> None:
        (repo / source).write_bytes(raw)
        digest = _sha(raw)
        next(
            item for item in lock["authorityBindings"] if item["sourcePath"] == source
        )["sha256"] = digest
        next(item for item in lock["trackedFiles"] if item["sourcePath"] == source)[
            "sha256"
        ] = digest

    def _validate_fixture(
        self,
        lock: dict[str, object],
        raw: bytes,
        repo: pathlib.Path,
        store: pathlib.Path,
        *,
        require_complete: bool = True,
    ) -> dict[str, object]:
        return rootfs.validate_source_lock(
            lock,
            raw,
            repo,
            store,
            require_complete=require_complete,
            trusted_ubuntu_fingerprints=self._trusted_fingerprints,
        )

    def _build_fixture(
        self,
        lock: dict[str, object],
        raw: bytes,
        repo: pathlib.Path,
        store: pathlib.Path,
        output: pathlib.Path,
    ) -> dict[str, object]:
        return rootfs.build_oci_layout(
            lock,
            raw,
            repo,
            store,
            output,
            trusted_ubuntu_fingerprints=self._trusted_fingerprints,
        )

    def _verify_fixture(
        self,
        lock: dict[str, object],
        raw: bytes,
        repo: pathlib.Path,
        store: pathlib.Path,
        output: pathlib.Path,
    ) -> dict[str, object]:
        return rootfs.verify_oci_layout(
            lock,
            raw,
            repo,
            store,
            output,
            trusted_ubuntu_fingerprints=self._trusted_fingerprints,
        )

    def test_tracked_lock_is_canonical_valid_incomplete_and_build_refuses(self) -> None:
        raw = TRACKED_LOCK.read_bytes()
        lock = rootfs.load_json_exact(raw, "tracked lock", require_canonical=True)
        with tempfile.TemporaryDirectory() as tmp:
            store = pathlib.Path(tmp) / "store"
            (store / "sha256").mkdir(parents=True)
            result = rootfs.validate_source_lock(
                lock, raw, ROOT, store, require_complete=False
            )
            self.assertFalse(result["sourceClosureComplete"])
            self.assertEqual(result["authorityStatus"], "UBUNTU-DEB-CLOSURE-NOT-RESOLVED")
            with self.assertRaises(rootfs.RootfsBuildError):
                rootfs.build_oci_layout(lock, raw, ROOT, store, pathlib.Path(tmp) / "oci")

    def test_complete_lock_binds_actual_builder_identity_and_authority_bytes(self) -> None:
        for scenario in ("builder", "identity", "activation"):
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as tmp:
                base = pathlib.Path(tmp)
                lock, _, repo, store = self._fixture(base)
                if scenario == "builder":
                    lock["buildRecipe"]["builderSha256"] = "00" * 32
                elif scenario == "identity":
                    (repo / "native/containment/native-shadow-toolchain-identity-v1.json").write_bytes(
                        b"{}\n"
                    )
                else:
                    lock["activationAllowed"] = True
                raw = rootfs.canonical_json(lock)
                with self.assertRaises(rootfs.RootfsBuildError):
                    self._validate_fixture(lock, raw, repo, store)

    def test_canonical_numeric_authority_and_closure_contracts_are_exact(self) -> None:
        scenarios = (
            "reordered-json",
            "float-limit",
            "missing-authority",
            "activated-policy",
            "closure-root",
        )
        for scenario in scenarios:
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as tmp:
                lock, _, repo, store = self._fixture(pathlib.Path(tmp))
                if scenario == "reordered-json":
                    reordered = dict(reversed(list(lock.items())))
                    raw = (json.dumps(reordered, indent=2) + "\n").encode("utf-8")
                    with self.assertRaises(rootfs.RootfsBuildError):
                        rootfs.load_json_exact(raw, "reordered lock", require_canonical=True)
                    continue
                if scenario == "float-limit":
                    lock["buildRecipe"]["maxEntries"] = 200000.0
                elif scenario == "missing-authority":
                    lock["authorityBindings"] = [
                        item
                        for item in lock["authorityBindings"]
                        if item["id"] != "checker-policy"
                    ]
                    lock["trackedFiles"] = [
                        item
                        for item in lock["trackedFiles"]
                        if not item["sourcePath"].endswith("/policy.json")
                    ]
                elif scenario == "activated-policy":
                    self._replace_authority(
                        lock,
                        repo,
                        "native/checker/rust-tuple-struct-project-v1/policy.json",
                        rootfs.canonical_json({"activationAllowed": True}),
                    )
                else:
                    lock["closureRoots"][0]["logicalRoots"] = ["/usr/bin/python3.12"]
                raw = rootfs.canonical_json(lock)
                with self.assertRaises(rootfs.RootfsBuildError):
                    self._validate_fixture(lock, raw, repo, store)

    def test_signed_ubuntu_metadata_and_dependency_closure_are_mandatory(self) -> None:
        for scenario in (
            "inrelease-tamper",
            "packages-tamper",
            "missing-resolution",
            "unreachable-package",
        ):
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as tmp:
                lock, _, repo, store = self._fixture(pathlib.Path(tmp))
                if scenario == "inrelease-tamper":
                    self._replace_artifact(
                        lock,
                        store,
                        "ubuntu-inrelease",
                        TEST_INRELEASE.replace(b"Codename: noble", b"Codename: foble", 1),
                    )
                elif scenario == "packages-tamper":
                    artifact = next(
                        item for item in lock["artifacts"]
                        if item["id"] == "ubuntu-packages-index"
                    )
                    raw_index = (store / "sha256" / artifact["sha256"]).read_bytes()
                    self._replace_artifact(
                        lock,
                        store,
                        "ubuntu-packages-index",
                        raw_index.replace(b"Version: 1.0-1", b"Version: 2.0-1", 1),
                    )
                elif scenario == "missing-resolution":
                    package = next(
                        item for item in lock["ubuntu"]["packages"]
                        if item["name"] == "gcc-13-x86-64-linux-gnu"
                    )
                    package["dependencyResolutions"] = []
                else:
                    lock["ubuntu"]["seeds"] = ["python3.12"]
                    lock["ubuntu"]["seedPackageIds"] = ["python3.12"]
                raw = rootfs.canonical_json(lock)
                with self.assertRaises(rootfs.RootfsBuildError):
                    self._validate_fixture(lock, raw, repo, store)

    def test_declared_seed_must_be_the_selected_root_package(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            lock, _, repo, store = self._fixture(pathlib.Path(tmp))
            # The compiler package depends on python, so graph reachability alone
            # would otherwise let an unrelated root impersonate the declared seed.
            lock["ubuntu"]["seeds"] = [
                "binutils-x86-64-linux-gnu",
                "gcc-13-x86-64-linux-gnu",
                "libc6-dev",
            ]
            lock["ubuntu"]["seedPackageIds"] = [
                "binutils-x86-64-linux-gnu",
                "gcc-13-x86-64-linux-gnu",
                "python3.12",
            ]
            raw = rootfs.canonical_json(lock)
            with self.assertRaisesRegex(rootfs.RootfsBuildError, "seed package roots"):
                self._validate_fixture(lock, raw, repo, store)

    def test_inrelease_uses_the_exact_gpgv_bytes_that_were_hashed(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            pinned = pathlib.Path(os.path.realpath(tmp)) / "gpgv"
            pinned.write_bytes(self._gpgv.read_bytes())
            pinned.chmod(0o555)
            frozen = rootfs._read_absolute_executable(str(pinned), _sha(pinned.read_bytes()))
            pinned.chmod(0o755)
            pinned.write_bytes(b"#!/bin/sh\nexit 99\n")
            pinned.chmod(0o555)
            payload = rootfs._verify_inrelease(
                frozen,
                TEST_KEYRING,
                TEST_INRELEASE,
                self._trusted_fingerprints,
                rootfs._snapshot_time("2026-08-23T23:00:00Z"),
            )
            self.assertIn(b"Codename: noble", payload)

    def test_debian_version_comparison_covers_epoch_tilde_and_revision(self) -> None:
        ordered = [
            "1.0~rc1-1",
            "1.0-1",
            "1.0-2",
            "1:0.1-1",
        ]
        for left, right in zip(ordered, ordered[1:]):
            self.assertLess(rootfs._debian_version_compare(left, right), 0)
            self.assertGreater(rootfs._debian_version_compare(right, left), 0)
        self.assertEqual(rootfs._debian_version_compare("2:1.0-1", "2:1.0-1"), 0)

    def test_two_direct_archive_builds_are_byte_identical_and_from_scratch(self) -> None:
        with tempfile.TemporaryDirectory() as first_tmp, tempfile.TemporaryDirectory() as second_tmp:
            first = pathlib.Path(first_tmp)
            second = pathlib.Path(second_tmp)
            lock_a, raw_a, repo_a, store_a = self._fixture(first)
            lock_b, raw_b, repo_b, store_b = self._fixture(second)
            extra = store_b / "sha256" / ("ff" * 32)
            extra.write_bytes(b"unreferenced artifact cannot affect output")
            receipt_a = self._build_fixture(lock_a, raw_a, repo_a, store_a, first / "oci")
            receipt_b = self._build_fixture(lock_b, raw_b, repo_b, store_b, second / "oci")
            self.assertEqual(receipt_a, receipt_b)
            self.assertEqual(rootfs.directory_digest(first / "oci"), rootfs.directory_digest(second / "oci"))
            self.assertEqual(receipt_a["layerCount"], 1)
            self.assertEqual(receipt_a["parentLayerCount"], 0)
            self.assertFalse(receipt_a["activationAllowed"])
            self.assertFalse(receipt_a["productionByteProvenanceComplete"])
            manifest = json.loads(
                (first / "oci/ROOTFS-CONTENT-MANIFEST.json").read_text(encoding="utf-8")
            )
            paths = {item["logicalPath"]: item for item in manifest["entries"]}
            for required in (
                "/opt/boole/native-checker-toolchain/bin/rustc",
                "/opt/boole/native-checker-toolchain/bin/cargo",
                "/usr/bin/python3.12",
                "/usr/lib/python3.12/os.py",
                "/usr/bin/ld",
                "/lib/x86_64-linux-gnu/libc.so.6",
                "/usr/share/boole/native-shadow/checkers/rust-tuple-struct-project-v1/checker.py",
            ):
                self.assertIn(required, paths)
            self.assertNotIn("SENTINEL-MUST-NOT-RUN", (first / "oci/ROOTFS-CONTENT-MANIFEST.json").read_text())
            layer_blob = first / "oci/blobs/sha256" / receipt_a["layerDigest"].split(":", 1)[1]
            self.assertNotIn(b"SENTINEL-MUST-NOT-RUN", layer_blob.read_bytes())
            self.assertNotIn(
                b"RUST-INSTALL-SCRIPT-MUST-NOT-COPY-OR-RUN", layer_blob.read_bytes()
            )

    def test_artifact_missing_digest_size_and_symlink_are_rejected(self) -> None:
        mutators = (
            lambda lock, store: next((store / "sha256" / item["sha256"]).unlink() for item in lock["artifacts"]),
            lambda lock, store: next((store / "sha256" / item["sha256"]).write_bytes(b"tampered") for item in lock["artifacts"]),
            lambda lock, store: next(item.__setitem__("sizeBytes", item["sizeBytes"] + 1) for item in lock["artifacts"]),
            lambda lock, store: (
                (store / "sha256" / lock["artifacts"][0]["sha256"]).unlink(),
                (store / "sha256" / lock["artifacts"][0]["sha256"]).symlink_to("../elsewhere"),
            ),
        )
        for mutate in mutators:
            with self.subTest(mutate=mutate), tempfile.TemporaryDirectory() as tmp:
                lock, _, repo, store = self._fixture(pathlib.Path(tmp))
                mutate(lock, store)
                raw = rootfs.canonical_json(lock)
                with self.assertRaises(rootfs.RootfsBuildError):
                    self._validate_fixture(lock, raw, repo, store)

    def test_deb_traversal_special_setid_pax_and_link_escape_are_rejected(self) -> None:
        bad_entries = (
            [{"name": "../escape", "raw": b"x"}],
            [{"name": "/absolute", "raw": b"x"}],
            [{"name": "./fifo", "kind": "fifo"}],
            [{"name": "./setuid", "raw": b"x", "mode": stat.S_ISUID | 0o755}],
            [{"name": "./usr/bin/link", "kind": "symlink", "target": "../../../escape"}],
            [{"name": "./usr/bin/pax", "raw": b"x", "pax": {"SCHILY.xattr.user.bad": "1"}}],
        )
        for entries in bad_entries:
            with self.subTest(entries=entries), tempfile.TemporaryDirectory() as tmp:
                base = pathlib.Path(tmp)
                lock, _, repo, store = self._fixture(base)
                self._replace_package(lock, store, "python3.12", self._deb(entries))
                raw = rootfs.canonical_json(lock)
                with self.assertRaises(rootfs.RootfsBuildError):
                    self._build_fixture(lock, raw, repo, store, base / "oci")

    def test_deb_preload_whiteout_and_symlink_cycles_are_rejected_directly(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            lock, _, _, _ = self._fixture(pathlib.Path(tmp))
            recipe = lock["buildRecipe"]
            bad_archives = (
                self._deb([{"name": "./etc/ld.so.preload", "raw": b"/evil.so\n"}]),
                self._deb([{"name": "./usr/lib/.wh.bad", "raw": b"x"}]),
            )
            for raw in bad_archives:
                with self.assertRaises(rootfs.RootfsBuildError):
                    rootfs._deb_payload(raw, "bad package", recipe)
            cycle = self._deb(
                [
                    {"name": "./usr/lib/cycle-a", "kind": "symlink", "target": "cycle-b"},
                    {"name": "./usr/lib/cycle-b", "kind": "symlink", "target": "cycle-a"},
                ]
            )
            entries = rootfs._deb_payload(cycle, "cycle package", recipe)
            rootfs._ensure_parents(entries)
            with self.assertRaises(rootfs.RootfsBuildError):
                rootfs._verify_link_graph(entries)
            with self.assertRaisesRegex(rootfs.RootfsBuildError, "decompression limit"):
                rootfs._decompress_limited(
                    lzma.compress(b"x" * 1024 * 1024),
                    "compression bomb",
                    1024,
                    compression="xz",
                )

    def test_tar_member_limit_counts_root_entries_that_normalize_away(self) -> None:
        archive = self._tar(
            ([{"name": ".", "kind": "directory"}] * 5)
            + [{"name": "x", "raw": b"x"}]
        )
        recipe = {"maxEntries": 1, "maxFileBytes": 1024, "maxTotalBytes": 1024}
        with self.assertRaisesRegex(rootfs.RootfsBuildError, "entry limit"):
            rootfs._tar_entries(archive, "member-bomb", recipe)

    def test_output_layer_refuses_unmaterialized_forward_hardlink(self) -> None:
        entries = {
            "a": {"kind": "hardlink", "mode": 0o444, "target": "z"},
            "z": {"kind": "file", "mode": 0o444, "raw": b"payload"},
        }
        with self.assertRaisesRegex(rootfs.RootfsBuildError, "materialized"):
            rootfs._layer_bytes(entries, 0)

    def test_conflicting_package_collision_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            lock, _, repo, store = self._fixture(base)
            self._replace_package(
                lock,
                store,
                "python3.12",
                self._deb([{"name": "./usr/bin/ld", "raw": b"different", "mode": 0o555}]),
            )
            raw = rootfs.canonical_json(lock)
            with self.assertRaises(rootfs.RootfsBuildError):
                self._build_fixture(lock, raw, repo, store, base / "oci")

    def test_noncanonical_unknown_and_duplicate_lock_are_rejected_by_cli(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            lock, raw, repo, store = self._fixture(base)
            lock_path = base / "lock.json"
            lock_path.write_bytes(json.dumps(lock).encode("utf-8"))
            script = pathlib.Path(rootfs.__file__).resolve()
            completed = subprocess.run(
                [
                    sys.executable,
                    str(script),
                    "validate-lock",
                    "--lock",
                    str(lock_path),
                    "--artifact-store",
                    str(store),
                    "--repo-root",
                    str(repo),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(completed.returncode, 2)
            self.assertNotIn("Traceback", completed.stderr)
            lock_path.write_bytes(raw.replace(b'"schema":', b'"schema":"duplicate",\n  "schema":', 1))
            duplicate = subprocess.run(
                [
                    sys.executable,
                    str(script),
                    "validate-lock",
                    "--lock",
                    str(lock_path),
                    "--artifact-store",
                    str(store),
                    "--repo-root",
                    str(repo),
                ],
                check=False,
                text=True,
                capture_output=True,
            )
            self.assertEqual(duplicate.returncode, 2)
            self.assertNotIn("Traceback", duplicate.stderr)

    def test_rootfs_content_manifest_has_real_distinct_provenance_closures(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            lock, raw, repo, store = self._fixture(base)
            self._build_fixture(lock, raw, repo, store, base / "oci")
            content = json.loads(
                (base / "oci/ROOTFS-CONTENT-MANIFEST.json").read_text(encoding="utf-8")
            )
            rustc = next(item for item in content["entries"] if item["logicalPath"].endswith("/bin/rustc"))
            python = next(item for item in content["entries"] if item["logicalPath"] == "/usr/bin/python3.12")
            libc = next(item for item in content["entries"] if item["logicalPath"].endswith("/libc.so.6"))
            self.assertEqual(rustc["closures"], [rootfs.REQUIRED_PROVENANCE_CLOSURES[0]])
            self.assertIn(rootfs.REQUIRED_PROVENANCE_CLOSURES[1], python["closures"])
            self.assertIn(rootfs.REQUIRED_PROVENANCE_CLOSURES[2], libc["closures"])
            self.assertNotEqual(rustc["closures"], python["closures"])

    def test_verify_rebuild_rejects_any_oci_layout_drift(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            lock, raw, repo, store = self._fixture(base)
            receipt = self._build_fixture(lock, raw, repo, store, base / "oci")
            self.assertEqual(
                self._verify_fixture(lock, raw, repo, store, base / "oci"),
                receipt,
            )
            index = base / "oci/index.json"
            index.chmod(0o644)
            index.write_bytes(index.read_bytes().replace(b'"schemaVersion":2', b'"schemaVersion":1'))
            with self.assertRaises(rootfs.RootfsBuildError):
                self._verify_fixture(lock, raw, repo, store, base / "oci")

    def test_verify_validates_lock_before_reading_layout(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            lock, _, repo, store = self._fixture(base)
            malformed = dict(lock)
            malformed.pop("buildRecipe")
            raw = rootfs.canonical_json(malformed)
            missing_layout = base / "must-not-be-read"
            with self.assertRaisesRegex(rootfs.RootfsBuildError, "source lock keys differ"):
                self._verify_fixture(malformed, raw, repo, store, missing_layout)

    def test_verify_rejects_layout_symlink_and_boolean_receipt_counts(self) -> None:
        for scenario in ("symlink", "boolean-count"):
            with self.subTest(scenario=scenario), tempfile.TemporaryDirectory() as tmp:
                base = pathlib.Path(tmp)
                lock, raw, repo, store = self._fixture(base)
                self._build_fixture(lock, raw, repo, store, base / "oci")
                if scenario == "symlink":
                    index = base / "oci/index.json"
                    outside = base / "outside-index.json"
                    outside.write_bytes(index.read_bytes())
                    index.unlink()
                    index.symlink_to(outside)
                else:
                    receipt_path = base / "oci/BUILD-RECEIPT.json"
                    receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
                    receipt["layerCount"] = True
                    receipt_path.chmod(0o644)
                    receipt_path.write_bytes(rootfs.canonical_json(receipt))
                with self.assertRaises(rootfs.RootfsBuildError):
                    self._verify_fixture(lock, raw, repo, store, base / "oci")


if __name__ == "__main__":
    unittest.main()
