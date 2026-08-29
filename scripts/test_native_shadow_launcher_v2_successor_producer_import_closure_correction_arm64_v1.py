#!/usr/bin/env python3
"""Pin the append-only repository-Python import-closure correction."""

from __future__ import annotations

import ast
import hashlib
import json
import os
import pathlib
import stat
import subprocess
import sys
import unittest


REPO = pathlib.Path(__file__).resolve().parents[1]
PREDECESSOR = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-"
    "preregistration-arm64-v1.json"
)
CORRECTION = (
    REPO
    / "native/containment/"
    "native-shadow-mac3-launcher-v2-successor-producer-"
    "import-closure-correction-arm64-v1.json"
)

ENTRYPOINTS = (
    "scripts/native_shadow_boot_image_verify_arm64_v1.py",
    "scripts/native_shadow_boot_root_disk_readback_arm64_v1.py",
    "scripts/native_shadow_boot_staging_measure_arm64_v1.py",
    "scripts/native_shadow_rootfs_builder_boot_arm64_v1.py",
    "scripts/native_shadow_rootfs_builder_boot_arm64_v4.py",
    "scripts/native_shadow_rootfs_portable_boot_arm64_v2.py",
)

RUNTIME_ONLY_MODULE = "scripts/native_shadow_rootfs_acquire_arm64_v1.py"
RUNTIME_DIRECT_DATA = (
    "native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json",
    "native/containment/native-shadow-boot-rootfs-closure-exception-arm64-v1.json",
    "native/containment/native-shadow-launcher-build-result-arm64-v2.json",
)
RUNTIME_TRANSITIVE_INPUTS = {
    "scripts/native_shadow_rootfs_acquire.py": {
        "verifiedBy": "scripts/native_shadow_rootfs_acquire_arm64_v1.py",
        "embeddedSha256": "31348981687939ff7cf63b5584947b3e09a92bb35f9f4e76f78a657ae139d49b",
    },
    "scripts/native_shadow_rootfs_builder.py": {
        "verifiedBy": "scripts/native_shadow_rootfs_builder_arm64_v1.py",
        "embeddedSha256": "aa25701a8a29cfb0059c911a5df8dcc2f09c8b4c61b4ff46adfc0ef446cdf689",
    },
    "scripts/native_shadow_rootfs_portable_v2.py": {
        "verifiedBy": "scripts/native_shadow_rootfs_portable_arm64_v1.py",
        "embeddedSha256": "11fe7f5672655cbfcf88e830d34ccc5b35274857df06cc123ed05e775bcd4fc3",
    },
}


def sha256(path: pathlib.Path) -> str:
    found = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            found.update(block)
    return found.hexdigest()


def load(path: pathlib.Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def imported_repo_modules(relative: str) -> set[str]:
    tree = ast.parse((REPO / relative).read_text(encoding="utf-8"))
    found: set[str] = set()
    for node in ast.walk(tree):
        modules: list[str] = []
        if isinstance(node, ast.ImportFrom) and node.module == "scripts":
            modules.extend(f"scripts.{alias.name}" for alias in node.names)
        elif (
            isinstance(node, ast.ImportFrom)
            and node.module is not None
            and node.module.startswith("scripts.")
        ):
            modules.append(node.module)
        elif isinstance(node, ast.Import):
            modules.extend(
                alias.name
                for alias in node.names
                if alias.name.startswith("scripts.")
            )
        for module in modules:
            candidate = module.replace(".", "/") + ".py"
            if (REPO / candidate).is_file():
                found.add(candidate)
    return found


def recursive_import_closure() -> set[str]:
    closure: set[str] = set()
    pending = list(ENTRYPOINTS)
    while pending:
        relative = pending.pop()
        if relative in closure:
            continue
        closure.add(relative)
        pending.extend(sorted(imported_repo_modules(relative) - closure))
    return closure


def runtime_import_evidence() -> dict[str, list[str]]:
    """Import the six entrypoints fresh and report repository reads."""

    modules = [path.removesuffix(".py").replace("/", ".") for path in ENTRYPOINTS]
    program = r'''
import importlib
import json
import pathlib
import sys

root = pathlib.Path.cwd().resolve()
opened = []

def audit(event, args):
    if event == "open" and args and isinstance(args[0], (str, bytes)):
        opened.append(args[0].decode() if isinstance(args[0], bytes) else args[0])

sys.addaudithook(audit)
for name in json.loads(sys.argv[1]):
    importlib.import_module(name)

repo_modules = sorted({
    pathlib.Path(module.__file__).resolve().relative_to(root).as_posix()
    for module in sys.modules.values()
    if getattr(module, "__file__", None)
    and pathlib.Path(module.__file__).resolve().is_relative_to(root)
})
repo_opens = set()
for raw in opened:
    path = pathlib.Path(raw).resolve()
    if path.is_relative_to(root):
        relative = path.relative_to(root).as_posix()
        if not relative.endswith(".pyc"):
            repo_opens.add(relative)
non_module_reads = sorted(repo_opens - set(repo_modules))
print("@@RUNTIME@@" + json.dumps({
    "modules": repo_modules,
    "nonModuleReads": non_module_reads,
}, sort_keys=True))
'''
    environment = dict(os.environ)
    environment["PYTHONDONTWRITEBYTECODE"] = "1"
    completed = subprocess.run(
        [sys.executable, "-c", program, json.dumps(modules)],
        cwd=REPO,
        env=environment,
        check=True,
        capture_output=True,
        text=True,
        timeout=30,
    )
    lines = [
        line.removeprefix("@@RUNTIME@@")
        for line in completed.stdout.splitlines()
        if line.startswith("@@RUNTIME@@")
    ]
    if len(lines) != 1:
        raise AssertionError(f"runtime audit emitted {len(lines)} evidence rows")
    return json.loads(lines[0])


class ImportClosureCorrectionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.predecessor = load(PREDECESSOR)
        self.record = load(CORRECTION)

    def test_record_is_canonical_append_only_and_authority_zero(self) -> None:
        canonical = (
            json.dumps(self.record, ensure_ascii=False, indent=2, sort_keys=True)
            + "\n"
        ).encode("utf-8")
        self.assertEqual(CORRECTION.read_bytes(), canonical)
        self.assertEqual(
            self.record["schema"],
            "boole.native-shadow.mac3.launcher-v2-successor-producer-"
            "import-closure-correction.arm64.v1",
        )
        self.assertEqual(
            self.record["status"],
            "CORRECTED-BEFORE-REHEARSAL-NO-IMAGE-PRODUCTION-AUTHORITY",
        )
        self.assertEqual(self.record["authorisations"], self.predecessor["authorisations"])
        self.assertEqual(self.record["runs"], self.predecessor["runs"])
        self.assertTrue(
            all(
                value is False
                for value in self.record["authorisations"].values()
                if type(value) is bool
            )
        )
        self.assertTrue(
            all(type(value) is int and value == 0 for value in self.record["runs"].values())
        )

    def test_predecessor_is_preserved_by_exact_identity(self) -> None:
        identity = self.record["predecessor"]
        self.assertEqual(
            identity,
            {
                "bindingCount": 23,
                "path": PREDECESSOR.relative_to(REPO).as_posix(),
                "preservedByteUnchanged": True,
                "sha256": "576bafd10600a05e9ab326e1e507c1a0351381d068f393ce402e295bf93afbec",
                "sizeBytes": 20145,
            },
        )
        self.assertEqual(sha256(PREDECESSOR), identity["sha256"])
        self.assertEqual(PREDECESSOR.stat().st_size, identity["sizeBytes"])

    def test_recursive_static_import_closure_is_exact_and_complete(self) -> None:
        closure = recursive_import_closure()
        self.assertEqual(len(closure), 22)
        self.assertEqual(
            sorted(closure), self.record["repositoryPythonImportClosure"]["paths"]
        )
        self.assertEqual(
            list(ENTRYPOINTS),
            self.record["repositoryPythonImportClosure"]["entrypoints"],
        )
        self.assertEqual(
            self.record["repositoryPythonImportClosure"]["algorithm"],
            "recursive Python AST Import and ImportFrom edges limited to live scripts.* modules",
        )

    def test_runtime_import_and_open_audit_closes_static_analysis_blind_spots(self) -> None:
        evidence = runtime_import_evidence()
        static = recursive_import_closure()
        self.assertEqual(set(evidence["modules"]), static | {RUNTIME_ONLY_MODULE})
        self.assertEqual(
            set(evidence["nonModuleReads"]),
            set(RUNTIME_DIRECT_DATA) | set(RUNTIME_TRANSITIVE_INPUTS),
        )
        sealed = self.record["repositoryPythonRuntimeClosure"]
        self.assertEqual(
            sealed["algorithm"],
            "fresh-process sys.modules plus Python audit-hook open events for all six entrypoints",
        )
        self.assertEqual(sealed["modules"], evidence["modules"])
        self.assertEqual(sealed["nonModuleReads"], evidence["nonModuleReads"])

    def test_legacy_exec_inputs_are_verified_transitively_before_compile(self) -> None:
        rows = self.record["transitivelyVerifiedInputs"]
        self.assertEqual({row["path"] for row in rows}, set(RUNTIME_TRANSITIVE_INPUTS))
        for row in rows:
            with self.subTest(path=row["path"]):
                expected = RUNTIME_TRANSITIVE_INPUTS[row["path"]]
                self.assertEqual(row["verifiedBy"], expected["verifiedBy"])
                self.assertEqual(row["sha256"], expected["embeddedSha256"])
                self.assertEqual(sha256(REPO / row["path"]), row["sha256"])
                wrapper = (REPO / row["verifiedBy"]).read_text(encoding="utf-8")
                self.assertIn(f'LEGACY_SHA256 = "{row["sha256"]}"', wrapper)
                self.assertLess(wrapper.index("LEGACY_SHA256"), wrapper.index("exec(compile("))

    def test_added_bindings_are_exactly_the_missing_eighteen_live_files(self) -> None:
        predecessor_paths = {row["path"] for row in self.predecessor["bindings"]}
        runtime = runtime_import_evidence()
        closure = set(runtime["modules"]) | set(RUNTIME_DIRECT_DATA)
        missing = closure - predecessor_paths
        rows = self.record["addedBindings"]
        self.assertEqual(len(missing), 18)
        self.assertEqual(len(rows), 18)
        self.assertEqual({row["path"] for row in rows}, missing)
        self.assertEqual(len({row["path"] for row in rows}), len(rows))
        for row in rows:
            with self.subTest(path=row["path"]):
                self.assertEqual(set(row), {"path", "role", "sha256", "sizeBytes"})
                self.assertTrue(row["role"])
                path = REPO / row["path"]
                info = path.lstat()
                self.assertTrue(stat.S_ISREG(info.st_mode))
                self.assertFalse(path.is_symlink())
                self.assertEqual(sha256(path), row["sha256"])
                self.assertEqual(info.st_size, row["sizeBytes"])

    def test_effective_union_is_forty_one_and_must_precede_import(self) -> None:
        effective = self.record["effectiveBinding"]
        self.assertEqual(
            effective,
            {
                "addedMissingBindings": 18,
                "bindingVerificationBeforeRepositoryPythonImport": True,
                "effectiveUniqueBindings": 41,
                "predecessorBindings": 23,
                "unionRequired": True,
            },
        )
        predecessor = {row["path"] for row in self.predecessor["bindings"]}
        added = {row["path"] for row in self.record["addedBindings"]}
        self.assertEqual(len(predecessor | added), 41)
        self.assertFalse(predecessor & added)

    def test_correction_is_before_every_run_and_never_rewrites_history(self) -> None:
        evidence = self.record["timingAndBoundary"]
        self.assertEqual(
            evidence,
            {
                "bootRunsPerformed": 0,
                "freeRehearsalsPerformed": 0,
                "imageProductionRunsPerformed": 0,
                "predecessorRecordRewritten": False,
                "producerImplementationMerged": False,
                "readbackImplementationMerged": False,
            },
        )
        self.assertIs(self.record["futureFingerprintMustBindBothRecords"], True)
        self.assertIs(self.record["supersedesForFutureImplementation"], True)
        self.assertIs(self.record["grantsAuthority"], False)


if __name__ == "__main__":
    unittest.main()
