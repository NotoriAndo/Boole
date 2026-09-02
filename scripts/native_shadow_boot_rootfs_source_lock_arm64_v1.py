#!/usr/bin/env python3
"""Seal the verified ARM64 package closure into a successor rootfs source lock.

The predecessor lock (``native-shadow-runtime-rootfs-source-lock-arm64-v1``)
covers the 56-package checker toolchain closure. It has no init system, no
launcher deployment bytes and no kernel seed, so it can never describe a
bootable guest. This tool derives the successor lock from material that is
already frozen and verified:

  * the dependency candidate result, which fixes 191 package IDs, pool paths,
    sizes and SHA-256 digests;
  * the payload acquisition result, which records that those exact payloads
    were fetched and verified against signed repository metadata;
  * the guest-init compatibility contract, which fixes the deployment bytes,
    the enablement rule and the ownership/mode of every tracked file;
  * the predecessor lock, whose checker closure is carried forward unchanged.

Nothing here builds, boots or activates anything. The ceiling verdict this
tool can reach is source shape: the declared inputs are present and internally
consistent. Runtime compatibility, boot authority and boot success are all
separate, later questions, and the boundaries in the result document say so.

The launcher binary is deliberately left open. Its guest placement (path,
mode, ownership) is bound here, but its digest is a build output of the ARM64
launcher build authority, which has not run. Writing a digest for a file that
does not exist would turn a plan into a false record, so the successor lock
omits the row and the audit reports the gap instead of hiding it.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
import tempfile
from typing import Any, Iterable

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_artifact_builder_arm64_v1 as boot
from scripts import native_shadow_guest_init_compatibility_arm64_v1 as guest_init

canonical_json = boot.canonical_json

CONTAINMENT = REPO_ROOT / "native" / "containment"
TOOL_PATH = pathlib.Path(__file__).resolve()
PLAN_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-plan-arm64-v1.json"
LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v1.json"
RESULT_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-result-arm64-v1.json"
BASELINE_LOCK_PATH = CONTAINMENT / "native-shadow-runtime-rootfs-source-lock-arm64-v1.json"
CANDIDATE_RESULT_PATH = (
    CONTAINMENT / "native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json"
)
ACQUISITION_RESULT_PATH = (
    CONTAINMENT / "native-shadow-boot-rootfs-payload-acquisition-result-arm64-v1.json"
)
CONTRACT_PATH = CONTAINMENT / "native-shadow-guest-init-compatibility-arm64-v1.json"

PLAN_SCHEMA = "boole.native-shadow.boot-rootfs-source-lock-plan.arm64.v1"
LOCK_SCHEMA = "boole.native-shadow.runtime-rootfs-source-lock.arm64.v1"
RESULT_SCHEMA = "boole.native-shadow.boot-rootfs-source-lock-result.arm64.v1"

LAUNCHER_BINARY_GUEST_PATH = "/usr/libexec/boole/boole-native-shadow-launcher"

# The candidate closure carries each package's payload digest and size inline.
# A source lock keeps payload identity in ``artifacts`` and package identity in
# ``ubuntu.packages``, so the closure rows are projected onto the lock's keys
# and the payload fields are compared against the artifact row instead.
LOCK_PACKAGE_KEYS = guest_init.UBUNTU_PACKAGE_KEYS

# The plan pins this tool by digest and this tool pins the plan by digest, so a
# literal digest cannot be part of its own preimage. The cycle is broken the
# same way the payload acquirer breaks it: the embedded plan digest is replaced
# by 64 zeros before the tool is hashed, so the pinned value is stable across
# re-sealing. A naive sha256 of this file therefore differs from the pin by
# design, not by tampering.
PLAN_SHA256 = "c047c20144167a4f28f222c4026a33e2d70b89340ee13cba79c207b7c92dc583"


class SourceLockError(RuntimeError):
    """A successor source lock failed a frozen acceptance ground."""


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def sha256_file(path: pathlib.Path) -> str:
    return sha256_bytes(path.read_bytes())


def source_lock_generator_authority_sha256(raw: bytes) -> str:
    """Digest this tool with its embedded plan digest normalized to zeros."""

    marker = b'PLAN_SHA256 = "'
    start = raw.index(marker) + len(marker)
    return sha256_bytes(raw[:start] + b"0" * 64 + raw[start + 64 :])


def _load_json(path: pathlib.Path, context: str) -> dict[str, Any]:
    try:
        raw = path.read_bytes()
    except FileNotFoundError as exc:
        raise SourceLockError(f"{context} is absent: {path}") from exc
    value = json.loads(raw.decode("utf-8"), object_pairs_hook=_no_duplicate_keys)
    if not isinstance(value, dict):
        raise SourceLockError(f"{context} is not a JSON object")
    if raw != canonical_json(value):
        raise SourceLockError(f"{context} is not canonical JSON: {path}")
    return value


def _no_duplicate_keys(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise SourceLockError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise SourceLockError(message)


def _sorted_by(rows: Iterable[dict[str, Any]], key: str) -> list[Any]:
    return sorted(row[key] for row in rows)


# ---------------------------------------------------------------------------
# frozen plan
# ---------------------------------------------------------------------------
def load_plan(path: pathlib.Path = PLAN_PATH) -> dict[str, Any]:
    plan = _load_json(path, "source lock plan")
    _require(plan.get("schema") == PLAN_SCHEMA, "source lock plan schema differs")
    _require(plan.get("activationAllowed") is False, "plan activationAllowed must stay false")
    _require(plan.get("bootableClaim") is False, "plan bootableClaim must stay false")
    for name, pin in plan["authorityInputs"].items():
        source = REPO_ROOT / pin["sourcePath"]
        observed = (
            source_lock_generator_authority_sha256(source.read_bytes())
            if name == "sourceLockGenerator"
            else sha256_file(source)
        )
        _require(
            observed == pin["sha256"],
            f"plan authority input digest differs: {name}",
        )
        _require(
            source.stat().st_size == pin["sizeBytes"],
            f"plan authority input size differs: {name}",
        )
    return plan


# ---------------------------------------------------------------------------
# generation
# ---------------------------------------------------------------------------
def _lock_package_row(row: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in row.items() if key in LOCK_PACKAGE_KEYS}


def build_source_lock(plan: dict[str, Any]) -> dict[str, Any]:
    baseline = _load_json(BASELINE_LOCK_PATH, "baseline source lock")
    candidate = _load_json(CANDIDATE_RESULT_PATH, "dependency candidate result")
    resolution = candidate["resolution"]
    packages = resolution["packages"]

    by_id = {row["packageId"]: row for row in packages}
    seed_ids = sorted(resolution["seedPackageIds"])
    seeds = sorted({by_id[package_id]["name"] for package_id in seed_ids})

    artifacts = [
        {
            "id": row["artifactId"],
            "kind": "deb",
            "sha256": row["artifactSha256"],
            "sizeBytes": row["artifactSizeBytes"],
        }
        for row in packages
    ]
    artifacts.extend(row for row in baseline["artifacts"] if row["kind"] != "deb")
    artifacts.sort(key=lambda row: row["id"])

    tracked = [dict(row) for row in plan["trackedFiles"]]
    for row in tracked:
        row.pop("role", None)
    tracked.sort(key=lambda row: row["logicalPath"])

    derived = [dict(row) for row in baseline["derivedEntries"]]
    derived.extend(dict(row) for row in plan["derivedEntries"])
    derived.sort(key=lambda row: row["logicalPath"])

    bindings = [dict(row) for row in plan["authorityBindings"]]
    bindings.sort(key=lambda row: row["id"])

    return {
        "activationAllowed": False,
        "artifacts": artifacts,
        "authorityBindings": bindings,
        "buildRecipe": baseline["buildRecipe"],
        "closureRoots": sorted(
            [*baseline["closureRoots"], *plan["closureRoots"]],
            key=lambda row: row["name"],
        ),
        "derivedEntries": derived,
        "platform": baseline["platform"],
        "release": plan["expected"]["lockRelease"],
        "rust": baseline["rust"],
        "schema": LOCK_SCHEMA,
        "trackedFiles": tracked,
        "ubuntu": {
            "packages": sorted(
                (_lock_package_row(row) for row in packages),
                key=lambda row: row["packageId"],
            ),
            "repositories": plan["repository"]["repositories"],
            "seedPackageIds": seed_ids,
            "seeds": seeds,
            "snapshot": plan["repository"]["snapshot"],
            "verification": plan["repository"]["verification"],
        },
    }


# ---------------------------------------------------------------------------
# verification
# ---------------------------------------------------------------------------
def verify_source_lock(
    plan: dict[str, Any],
    source_lock: dict[str, Any],
    *,
    verify_current_source_bytes: bool = True,
) -> dict[str, Any]:
    """Refuse a successor lock that fails any frozen acceptance ground.

    The strict default is retained for the historical producer.  Current-tree
    tests can validate the sealed plan/lock relationship without pretending
    that today's evolving source files are still that historical workspace.
    """

    _verify_identity(source_lock)
    _verify_build_recipe(plan, source_lock)
    _verify_repository(plan, source_lock)
    _verify_ordering(source_lock)
    _verify_package_closure(source_lock)
    _verify_seeds(source_lock)
    _verify_tracked_files(
        plan,
        source_lock,
        verify_current_source_bytes=verify_current_source_bytes,
    )
    _verify_derived_entries(plan, source_lock)
    _verify_authority_bindings(
        source_lock,
        verify_current_source_bytes=verify_current_source_bytes,
    )
    return _audit(source_lock)


def _verify_identity(source_lock: dict[str, Any]) -> None:
    _require(source_lock.get("schema") == LOCK_SCHEMA, "source lock schema differs")
    _require(
        source_lock.get("activationAllowed") is False,
        "source lock activationAllowed must stay false",
    )
    _require(
        source_lock.get("platform", {}).get("debArchitecture") == "arm64",
        "source lock platform is not arm64",
    )


def _verify_build_recipe(plan: dict[str, Any], source_lock: dict[str, Any]) -> None:
    recipe = source_lock.get("buildRecipe")
    _require(isinstance(recipe, dict), "source lock buildRecipe is absent")
    _require(
        recipe.get("maintainerScripts") == "never-execute-or-copy",
        "source lock permits maintainer script execution",
    )
    _require(
        recipe.get("network") == "forbidden",
        "source lock permits network during the build",
    )
    _require(
        recipe == plan["buildRecipe"],
        "source lock buildRecipe differs from the frozen plan",
    )


def _verify_repository(plan: dict[str, Any], source_lock: dict[str, Any]) -> None:
    ubuntu = source_lock.get("ubuntu")
    _require(isinstance(ubuntu, dict), "source lock Ubuntu closure is absent")
    pinned = plan["repository"]
    _require(
        ubuntu.get("snapshot") == pinned["snapshot"],
        "source lock Ubuntu snapshot differs from the frozen snapshot",
    )
    _require(
        ubuntu.get("repositories") == pinned["repositories"],
        "source lock repository pin differs from the frozen repository",
    )
    _require(
        ubuntu.get("verification") == pinned["verification"],
        "source lock repository signature verification role differs",
    )
    repository_ids = {row["id"] for row in pinned["repositories"]}
    for row in ubuntu.get("packages", []):
        _require(
            row.get("repositoryId") in repository_ids,
            "source lock package comes from a repository outside the frozen snapshot",
        )


def _verify_ordering(source_lock: dict[str, Any]) -> None:
    artifacts = source_lock.get("artifacts")
    _require(isinstance(artifacts, list), "source lock artifacts are absent")
    ids = [row["id"] for row in artifacts]
    _require(ids == sorted(ids), "source lock artifacts are not sorted by id")
    _require(len(set(ids)) == len(ids), "source lock artifacts are duplicated")
    packages = source_lock["ubuntu"].get("packages")
    _require(isinstance(packages, list), "source lock packages are absent")
    package_ids = [row["packageId"] for row in packages]
    _require(
        package_ids == sorted(package_ids),
        "source lock packages are not sorted by packageId",
    )
    _require(len(set(package_ids)) == len(package_ids), "source lock packages are duplicated")


def _verify_package_closure(source_lock: dict[str, Any]) -> None:
    candidate = _load_json(CANDIDATE_RESULT_PATH, "dependency candidate result")
    acquisition = _load_json(ACQUISITION_RESULT_PATH, "payload acquisition result")
    _require(
        acquisition["candidateResultSha256"] == sha256_file(CANDIDATE_RESULT_PATH),
        "the acquisition result does not describe this candidate closure",
    )
    _require(
        acquisition["boundaries"]["packagePayloadsVerified"] is True,
        "the acquisition result does not report verified payloads",
    )
    expected = {row["packageId"]: row for row in candidate["resolution"]["packages"]}

    packages = source_lock["ubuntu"]["packages"]
    observed_ids = {row["packageId"] for row in packages}
    surplus = sorted(observed_ids - set(expected))
    absent = sorted(set(expected) - observed_ids)
    _require(
        not surplus,
        "source lock carries a package outside the verified package closure: "
        + ", ".join(surplus[:3]),
    )
    _require(
        not absent,
        "source lock drops a package from the verified package closure: "
        + ", ".join(absent[:3]),
    )
    for row in packages:
        _require(
            row == _lock_package_row(expected[row["packageId"]]),
            f"source lock package row differs from the verified package closure: {row['name']}",
        )

    acquired = set(acquisition["fetchedArtifactIds"]) | set(acquisition["reusedPackageIds"])
    artifacts = {row["id"]: row for row in source_lock["artifacts"]}
    for package_id, row in expected.items():
        _require(
            package_id in acquired,
            f"package payload was never acquired: {package_id}",
        )
        artifact = artifacts.get(row["artifactId"])
        _require(
            artifact is not None,
            f"artifact is absent for a package in the verified package closure: {row['name']}",
        )
        _require(
            artifact["kind"] == "deb",
            f"artifact kind differs from the verified package closure: {row['name']}",
        )
        _require(
            artifact["sha256"] == row["artifactSha256"],
            f"artifact sha256 differs from the verified package closure: {row['name']}",
        )
        _require(
            artifact["sizeBytes"] == row["artifactSizeBytes"],
            f"artifact sizeBytes differs from the verified package closure: {row['name']}",
        )


def _verify_seeds(source_lock: dict[str, Any]) -> None:
    ubuntu = source_lock["ubuntu"]
    seeds = ubuntu.get("seeds")
    _require(isinstance(seeds, list), "source lock package seeds are absent")
    _require(
        "systemd" in seeds,
        "source lock drops the systemd package seed, so the guest would have no init system",
    )
    by_id = {row["packageId"]: row for row in ubuntu["packages"]}
    seed_ids = ubuntu.get("seedPackageIds")
    _require(isinstance(seed_ids, list), "source lock seed package IDs are absent")
    _require(
        seed_ids == sorted(set(seed_ids)),
        "source lock seed package IDs are not sorted and unique",
    )
    _require(
        all(package_id in by_id for package_id in seed_ids),
        "source lock seed package ID is outside the package closure",
    )
    _require(
        sorted({by_id[package_id]["name"] for package_id in seed_ids}) == seeds,
        "source lock package seeds and seed package IDs differ",
    )


def _verify_tracked_files(
    plan: dict[str, Any],
    source_lock: dict[str, Any],
    *,
    verify_current_source_bytes: bool = True,
) -> None:
    rows = source_lock.get("trackedFiles")
    _require(isinstance(rows, list), "source lock trackedFiles are absent")
    required = {row["logicalPath"]: row for row in plan["trackedFiles"]}
    by_path: dict[str, dict[str, Any]] = {}
    for row in rows:
        path = row["logicalPath"]
        _require(
            "replay-node" not in path,
            f"source lock tracks a replay-node authority file: {path}",
        )
        _require(
            path != LAUNCHER_BINARY_GUEST_PATH,
            "source lock states a launcher binary digest, but the launcher binary is a build "
            "output of the ARM64 launcher build authority, which has not run",
        )
        _require(path in required, f"source lock tracks an unexpected file: {path}")
        _require(path not in by_path, f"source lock tracks a duplicated file: {path}")
        by_path[path] = row
    paths = list(by_path)
    _require(paths == sorted(paths), "source lock tracked files are not sorted")

    for path, want in required.items():
        role = want["role"]
        row = by_path.get(path)
        _require(row is not None, f"required tracked file is absent: {role}")
        for field in ("gid", "mode", "sha256", "sourcePath", "uid"):
            _require(
                row[field] == want[field],
                f"tracked file {field} differs: {role}",
            )
        if verify_current_source_bytes:
            source = REPO_ROOT / row["sourcePath"]
            _require(
                source.is_file(),
                f"tracked file source bytes are absent: {role}",
            )
            _require(
                sha256_file(source) == row["sha256"],
                f"tracked file source bytes differ from the pinned digest: {role}",
            )


def _verify_derived_entries(plan: dict[str, Any], source_lock: dict[str, Any]) -> None:
    rows = source_lock.get("derivedEntries")
    _require(isinstance(rows, list), "source lock derivedEntries are absent")
    by_path: dict[str, dict[str, Any]] = {}
    for row in rows:
        path = row["logicalPath"]
        target = row.get("target", "")
        _require(
            "replay-node" not in path and "replay-node" not in target,
            f"source lock enables a replay-node authority unit: {path}",
        )
        _require(path not in by_path, f"source lock derives a duplicated entry: {path}")
        by_path[path] = row
    paths = list(by_path)
    _require(paths == sorted(paths), "source lock derived entries are not sorted")

    for want in plan["derivedEntries"]:
        role = plan["expected"]["derivedEntryRoles"][want["logicalPath"]]
        row = by_path.get(want["logicalPath"])
        _require(row is not None, f"required derived entry is absent: {role}")
        _require(
            row == want,
            f"required derived entry differs: {role}",
        )


def _verify_authority_bindings(
    source_lock: dict[str, Any],
    *,
    verify_current_source_bytes: bool = True,
) -> None:
    rows = source_lock.get("authorityBindings")
    _require(isinstance(rows, list), "source lock authority bindings are absent")
    ids = [row["id"] for row in rows]
    _require(ids == sorted(set(ids)), "source lock authority binding IDs are not sorted and unique")
    for row in rows:
        if verify_current_source_bytes:
            source = REPO_ROOT / row["sourcePath"]
            _require(
                source.is_file(),
                f"source lock authority binding source is absent: {row['id']}",
            )
            _require(
                sha256_file(source) == row["sha256"],
                f"source lock authority binding digest differs from the file on disk: {row['id']}",
            )
    bound = {(row["sourcePath"], row["sha256"]) for row in rows}
    for row in source_lock["trackedFiles"]:
        _require(
            (row["sourcePath"], row["sha256"]) in bound,
            f"tracked file is not covered by an authority binding: {row['logicalPath']}",
        )


def _audit(source_lock: dict[str, Any]) -> dict[str, Any]:
    """Run the frozen guest-init contract's own successor source-shape audit."""

    with tempfile.TemporaryDirectory() as scratch:
        candidate = pathlib.Path(scratch) / "successor-source-lock.json"
        candidate.write_bytes(canonical_json(source_lock))
        try:
            return guest_init.audit_successor_source_shape(CONTRACT_PATH, candidate)
        except guest_init.GuestInitCompatibilityError as exc:
            raise SourceLockError(f"guest-init source shape audit refused the lock: {exc}") from exc


# ---------------------------------------------------------------------------
# result
# ---------------------------------------------------------------------------
def build_result(
    plan: dict[str, Any], source_lock: dict[str, Any], audit: dict[str, Any]
) -> dict[str, Any]:
    acquisition = _load_json(ACQUISITION_RESULT_PATH, "payload acquisition result")
    packages = source_lock["ubuntu"]["packages"]
    payload_bytes = {row["id"]: row["sizeBytes"] for row in source_lock["artifacts"]}
    return {
        "activationAllowed": False,
        "bootArtifactsWritten": 0,
        "bootableClaim": False,
        "boundaries": {
            "bootAuthority": False,
            "guestBootVerified": False,
            "imageBuilderAuthorityPresent": False,
            "kernelImageExtracted": False,
            "launcherElfPresent": False,
            "maintainerScriptsExecuted": False,
            "runtimeCompatibilityVerified": False,
        },
        "counts": {
            "artifacts": len(source_lock["artifacts"]),
            "authorityBindings": len(source_lock["authorityBindings"]),
            "derivedEntries": len(source_lock["derivedEntries"]),
            "packageBytes": sum(payload_bytes[row["artifactId"]] for row in packages),
            "packages": len(packages),
            "seedPackages": len(source_lock["ubuntu"]["seedPackageIds"]),
            "trackedFiles": len(source_lock["trackedFiles"]),
        },
        "deferredRoles": [
            {
                "cause": (
                    "the guest launcher ELF is a build output of the ARM64 launcher build "
                    "authority, which has not run; a digest cannot be stated for a file that "
                    "does not exist"
                ),
                "guestPath": LAUNCHER_BINARY_GUEST_PATH,
                "resolvedBy": "arm64-launcher-build-authority",
                "role": "tracked-file:launcher-binary",
            }
        ],
        "planSha256": PLAN_SHA256,
        "predecessorSourceLockSha256": sha256_file(BASELINE_LOCK_PATH),
        "payloadAcquisitionResultSha256": sha256_file(ACQUISITION_RESULT_PATH),
        "payloadsVerified": acquisition["boundaries"]["packagePayloadsVerified"],
        "productionByteProvenanceComplete": False,
        "schema": RESULT_SCHEMA,
        "signedRepositoryMetadataVerified": acquisition["signedRepositoryMetadataVerified"],
        "sourceLockSha256": audit["sourceLockSha256"],
        "sourceShapeAudit": {
            "missingRoles": audit["missingRoles"],
            "status": audit["status"],
        },
        "status": plan["expected"]["resultStatus"],
    }


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true", help="verify the sealed documents")
    group.add_argument("--write", action="store_true", help="regenerate the sealed documents")
    args = parser.parse_args(argv)

    plan = load_plan()
    _require(
        PLAN_SHA256 == sha256_file(PLAN_PATH),
        "this tool pins a different plan digest than the plan on disk",
    )
    source_lock = build_source_lock(plan)
    audit = verify_source_lock(plan, source_lock)
    _require(
        audit["status"] == plan["expected"]["auditStatus"],
        f"source shape audit status differs: {audit['status']}",
    )
    _require(
        audit["missingRoles"] == plan["expected"]["auditMissingRoles"],
        f"source shape audit missing roles differ: {audit['missingRoles']}",
    )
    result = build_result(plan, source_lock, audit)

    documents = ((LOCK_PATH, source_lock), (RESULT_PATH, result))
    if args.write:
        for path, document in documents:
            path.write_bytes(canonical_json(document))
        print(f"source lock written: {LOCK_PATH.name} ({audit['sourceLockSha256']})")
        return 0

    for path, document in documents:
        _require(path.is_file(), f"sealed document is absent: {path.name}")
        _require(
            path.read_bytes() == canonical_json(document),
            f"sealed document differs from the regenerated document: {path.name}",
        )
    print(
        f"source lock: {audit['sourceLockSha256']} "
        f"status={audit['status']} missingRoles={audit['missingRoles']}"
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except SourceLockError as error:
        print(f"source lock refused: {error}", file=sys.stderr)
        sys.exit(1)
