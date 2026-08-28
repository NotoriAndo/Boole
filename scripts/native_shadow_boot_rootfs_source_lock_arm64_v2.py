#!/usr/bin/env python3
"""Derive the successor boot rootfs source lock from the frozen plan successor.

The predecessor generator sealed ten tracked files. Three things were missing
from the image it described, and the serving-gap closure plan fixed the order in
which they get closed:

  * the account database the guest's own sysusers file expects to fill in, which
    ``systemd-sysusers`` cannot create on a read-only root;
  * the already-sealed runtime rootfs and its content manifest, nested under a
    prefix inside the boot rootfs;
  * a channel by which the launcher's refusal can be read, which is two lines of
    the launcher unit.

This tool is the second of the four steps. It knows how to build and verify the
successor lock; it does not seal it. Sealing is the third step, and until that
step runs ``--check`` refuses rather than inventing a document.

Two properties are worth stating plainly, because they are what keeps a
successor from quietly becoming a relaxation.

The predecessor's acceptance grounds are not restated here. They are imported
from the predecessor generator and run against the successor lock, so a ground
cannot be weakened by rewording it -- the same code decides.

The frozen guest-init compatibility contract pins the digest of two files this
successor supersedes, so it necessarily refuses the successor lock. That refusal
is not routed around. A *shadow* lock is built alongside the real one, identical
except that the two superseded sources are restored to their sealed
predecessors, and the frozen contract must still return the predecessor's own
verdict for it. What that proves is narrow and exact: the successor is additive
everywhere except at the two supersessions this plan records, and it is those
two and no others.
"""
from __future__ import annotations

import argparse
import copy
import json
import pathlib
import sys
import tempfile
from typing import Any

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_artifact_builder_arm64_v1 as boot
from scripts import native_shadow_boot_rootfs_source_lock_arm64_v1 as predecessor
from scripts import native_shadow_guest_init_compatibility_arm64_v1 as guest_init

canonical_json = boot.canonical_json

# The predecessor's own acceptance grounds, run rather than reworded.
SourceLockError = predecessor.SourceLockError
sha256_bytes = predecessor.sha256_bytes
sha256_file = predecessor.sha256_file

CONTAINMENT = REPO_ROOT / "native" / "containment"
TOOL_PATH = pathlib.Path(__file__).resolve()
PLAN_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-plan-arm64-v2.json"
LOCK_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-arm64-v2.json"
RESULT_PATH = CONTAINMENT / "native-shadow-boot-rootfs-source-lock-result-arm64-v2.json"
PREDECESSOR_LOCK_PATH = predecessor.LOCK_PATH
PREDECESSOR_RESULT_PATH = predecessor.RESULT_PATH
BASELINE_LOCK_PATH = predecessor.BASELINE_LOCK_PATH
ACQUISITION_RESULT_PATH = predecessor.ACQUISITION_RESULT_PATH
CONTRACT_PATH = predecessor.CONTRACT_PATH
EXPECTATION_PATH = CONTAINMENT / "native-shadow-runtime-rootfs-replay-expectation-arm64-v1.json"
AUTHORITY_ARCH_PATH = REPO_ROOT / "crates/boole-native-shadow-launcher/src/authority_arch.rs"

PLAN_SCHEMA = "boole.native-shadow.boot-rootfs-source-lock-plan.arm64.v2"
LOCK_SCHEMA = predecessor.LOCK_SCHEMA
RESULT_SCHEMA = "boole.native-shadow.boot-rootfs-source-lock-result.arm64.v2"

LOCK_RELEASE = "NATIVE-SHADOW-BOOT-ROOTFS-SOURCE-LOCK-ARM64-V2-SOURCE-SHAPE-ONLY-NOT-BOOTABLE"
RESULT_STATUS = (
    "BOOT-ROOTFS-SOURCE-LOCK-SUCCESSOR-SEALED-LAUNCHER-BINARY-DEFERRED-NOT-BOOT-AUTHORITY"
)

# The plan is frozen, so it cannot pin this tool and this tool can pin it
# outright. The chain runs one way: the plan is pinned here, this tool is pinned
# in the result it produces, and no digest is part of its own preimage.
PLAN_SHA256 = "da4e7af1dd3cb1db9e263363210c1aec30b7f1bd60ddf87c73fa3921bc018777"
PLAN_SIZE_BYTES = 32252

# The eight clauses ``service_identities::resolve_one`` enforces in the guest,
# re-derived here from the source bytes rather than taken on the plan's word.
REQUIRED_HOME = "/nonexistent"
ALLOWED_SHELLS = ("/usr/sbin/nologin", "/bin/false")
FIXED_ACCOUNTS = ("boole-native-checker", "boole-node")

_ROOT_ONLY_MODE = "0400"
_PASSWORD_BEARING = ("/etc/gshadow", "/etc/shadow")


def _require(condition: bool, message: str) -> None:
    predecessor._require(condition, message)


def _load_json(path: pathlib.Path, context: str) -> dict[str, Any]:
    return predecessor._load_json(path, context)


# ---------------------------------------------------------------------------
# frozen plan
# ---------------------------------------------------------------------------
def load_plan(path: pathlib.Path = PLAN_PATH) -> dict[str, Any]:
    """Load the frozen plan successor and refuse one that claims more than it did."""

    plan = _load_json(path, "boot source lock plan successor")
    _require(plan.get("schema") == PLAN_SCHEMA, "source lock plan schema differs")
    _require(plan.get("activationAllowed") is False, "plan activationAllowed must stay false")
    built = plan.get("whatWasBuilt")
    _require(isinstance(built, dict), "plan whatWasBuilt is absent")
    for key, value in sorted(built.items()):
        _require(value is False, f"plan claims something was already built: {key}")
    _require(
        plan["successorChainPosition"]["step"] == 1,
        "plan is not the first step of the successor chain",
    )
    for record in plan["appendOnly"]["recordsLeftByteUnchanged"]:
        target = REPO_ROOT / record["path"]
        _require(target.is_file(), f"a record the plan left unchanged is absent: {record['path']}")
        raw = target.read_bytes()
        _require(
            sha256_bytes(raw) == record["sha256"] and len(raw) == record["sizeBytes"],
            f"a record the plan left unchanged has drifted: {record['path']}",
        )
    return plan


def deferred_roles(plan: dict[str, Any]) -> list[str]:
    """The roles the plan itself still defers, read back rather than restated."""

    return sorted(
        row["role"] for row in plan["guestInitRoles"] if row["state"] == "deferred"
    )


# ---------------------------------------------------------------------------
# generation
# ---------------------------------------------------------------------------
def build_source_lock(plan: dict[str, Any]) -> dict[str, Any]:
    """Assemble the successor lock with the predecessor's own assembly code.

    The predecessor reads the lock release out of ``expected.lockRelease``; the
    plan successor carries its own release at the top level and states the lock
    release nowhere, because a plan that names no lock should not name a lock's
    release either. The one field is supplied here and everything else is the
    predecessor's assembly, so the successor cannot drift in shape.
    """

    shim = dict(plan)
    shim["expected"] = {"lockRelease": LOCK_RELEASE}
    return predecessor.build_source_lock(shim)


def build_shadow_lock(plan: dict[str, Any], source_lock: dict[str, Any]) -> dict[str, Any]:
    """The successor lock with the two superseded sources restored.

    This document is never written. It exists so the frozen guest-init contract,
    which pins the digest of both superseded files, can still be asked its own
    question about everything the successor did not move.
    """

    shadow = copy.deepcopy(source_lock)
    by_old = {row["logicalPath"]: row for row in plan["changesFromPredecessor"]["supersessions"]}
    moved = {(row["newSourcePath"], row["newSha256"]): row for row in by_old.values()}
    for row in shadow["trackedFiles"]:
        want = by_old.get(row["logicalPath"])
        if want is None:
            continue
        row["sourcePath"] = want["oldSourcePath"]
        row["sha256"] = want["oldSha256"]
    for row in shadow["authorityBindings"]:
        want = moved.get((row["sourcePath"], row["sha256"]))
        if want is None:
            continue
        row["sourcePath"] = want["oldSourcePath"]
        row["sha256"] = want["oldSha256"]
    return shadow


# ---------------------------------------------------------------------------
# verification
# ---------------------------------------------------------------------------
def verify_source_lock(plan: dict[str, Any], source_lock: dict[str, Any]) -> dict[str, Any]:
    """Refuse a successor lock that fails any ground, inherited or new."""

    predecessor._verify_identity(source_lock)
    predecessor._verify_build_recipe(plan, source_lock)
    predecessor._verify_repository(plan, source_lock)
    predecessor._verify_ordering(source_lock)
    predecessor._verify_package_closure(source_lock)
    predecessor._verify_seeds(source_lock)
    predecessor._verify_tracked_files(plan, source_lock)
    predecessor._verify_authority_bindings(source_lock)

    _verify_release(source_lock)
    _verify_derived_entries(source_lock)
    _verify_bindings_are_one_for_one(source_lock)
    supersessions = _verify_supersessions(plan, source_lock)
    accounts = _verify_account_database(plan, source_lock)
    nested = _verify_nested_tree(plan, source_lock)
    predecessor_audit = _audit_shadow(plan, source_lock)
    missing = _missing_roles(plan, source_lock)

    _require(
        missing == deferred_roles(plan),
        f"successor missing roles differ from the roles the plan defers: {missing}",
    )
    return {
        "accountsVerified": accounts,
        "identityClausesVerified": len(plan["identityContractClauses"]),
        "missingRoles": missing,
        "nestedTree": nested,
        "predecessorContractAudit": predecessor_audit,
        "sourceLockSha256": sha256_bytes(canonical_json(source_lock)),
        "status": (
            "BLOCKED_MISSING_GUEST_INIT_REQUIREMENTS"
            if missing
            else "SOURCE_SHAPE_REQUIREMENTS_PRESENT_UNVERIFIED"
        ),
        "supersessionsVerified": supersessions,
    }


def _verify_release(source_lock: dict[str, Any]) -> None:
    release = source_lock.get("release")
    _require(release == LOCK_RELEASE, "successor lock release differs")
    _require(
        release
        != _load_json(PREDECESSOR_LOCK_PATH, "predecessor boot source lock")["release"],
        "successor lock reuses the predecessor release",
    )


def _verify_derived_entries(source_lock: dict[str, Any]) -> None:
    """Check the derived entries against the contract's own requirement rows.

    The predecessor read the role names out of its plan's ``expected`` block. The
    plan successor has no such block, and inventing one here would mean this tool
    deciding what the contract requires. The requirement rows are read from the
    contract instead.
    """

    contract = _load_json(CONTRACT_PATH, "guest-init compatibility contract")
    requirements = {row["logicalPath"]: row for row in contract["derivedEntryRequirements"]}
    rows = source_lock.get("derivedEntries")
    _require(isinstance(rows, list), "source lock derivedEntries are absent")
    paths = [row["logicalPath"] for row in rows]
    _require(paths == sorted(set(paths)), "source lock derived entries are not sorted and unique")
    for path, want in requirements.items():
        row = next((candidate for candidate in rows if candidate["logicalPath"] == path), None)
        _require(row is not None, f"required derived entry is absent: {want['role']}")
        _require(
            row["target"] == want["target"],
            f"required derived entry target differs: {want['role']}",
        )
        _require(
            row["uid"] == 0 and row["gid"] == 0,
            f"required derived entry is not root-owned: {want['role']}",
        )


def _verify_bindings_are_one_for_one(source_lock: dict[str, Any]) -> None:
    tracked = source_lock["trackedFiles"]
    bindings = source_lock["authorityBindings"]
    _require(
        len(bindings) == len(tracked),
        "successor lock has a binding that covers no tracked file, or a tracked file with none",
    )
    bound = {(row["sourcePath"], row["sha256"]) for row in bindings}
    named = {(row["sourcePath"], row["sha256"]) for row in tracked}
    _require(bound == named, "successor lock bindings and tracked sources are not the same set")


def _verify_supersessions(plan: dict[str, Any], source_lock: dict[str, Any]) -> list[str]:
    """Prove each moved row is a recorded supersession and not an edit."""

    contract = _load_json(CONTRACT_PATH, "guest-init compatibility contract")
    pinned = {row["logicalPath"]: row for row in contract["trackedFileRequirements"]}
    unchanged = {row["path"]: row for row in plan["appendOnly"]["recordsLeftByteUnchanged"]}
    tracked = {row["logicalPath"]: row for row in source_lock["trackedFiles"]}
    bindings = {row["id"]: row for row in source_lock["authorityBindings"]}
    predecessor_lock = _load_json(PREDECESSOR_LOCK_PATH, "predecessor boot source lock")
    predecessor_bindings = {row["id"]: row for row in predecessor_lock["authorityBindings"]}

    supersessions = plan["changesFromPredecessor"]["supersessions"]
    _require(
        len(supersessions) == plan["changesFromPredecessor"]["supersededTrackedSources"],
        "the plan's supersession count and its supersession rows differ",
    )
    verified: list[str] = []
    for row in supersessions:
        role = row["role"]
        path = row["logicalPath"]
        want = pinned.get(path)
        _require(want is not None, f"a supersession names a path the contract does not pin: {path}")
        _require(
            want["sha256"] == row["oldSha256"],
            f"a supersession's old digest is not the digest the contract pins: {role}",
        )
        old = unchanged.get(row["oldSourcePath"])
        _require(
            old is not None and old["sha256"] == row["oldSha256"],
            f"the superseded file is not kept at its sealed digest: {role}",
        )
        _require(
            sha256_file(REPO_ROOT / row["oldSourcePath"]) == row["oldSha256"],
            f"the superseded file on disk differs from its sealed digest: {role}",
        )
        successor_source = REPO_ROOT / row["newSourcePath"]
        _require(successor_source.is_file(), f"a supersession's successor file is absent: {role}")
        raw = successor_source.read_bytes()
        _require(
            sha256_bytes(raw) == row["newSha256"] and len(raw) == row["newSizeBytes"],
            f"a supersession's successor file differs from the plan: {role}",
        )
        _require(row["newSha256"] != row["oldSha256"], f"a supersession moves nothing: {role}")
        placed = tracked.get(path)
        _require(placed is not None, f"a superseded path is not tracked: {role}")
        _require(
            placed["sourcePath"] == row["newSourcePath"] and placed["sha256"] == row["newSha256"],
            f"the tracked row does not carry the successor source: {role}",
        )
        _require(
            placed["mode"] == want["mode"]
            and placed["uid"] == want["uid"]
            and placed["gid"] == want["gid"],
            f"a supersession moves the placement as well as the bytes: {role}",
        )
        inherited = bindings.get(role)
        _require(
            inherited is not None and role in predecessor_bindings,
            f"a supersession reissues the authority binding identity: {role}",
        )
        _require(
            inherited["sourcePath"] == row["newSourcePath"],
            f"the inherited binding does not name the successor source: {role}",
        )
        verified.append(role)
    return sorted(verified)


def _passwd_rows(raw: bytes) -> dict[str, list[str]]:
    rows: dict[str, list[str]] = {}
    for line in raw.decode("utf-8").splitlines():
        if not line.strip():
            continue
        fields = line.split(":")
        _require(len(fields) == 7, f"passwd line does not have seven fields: {line}")
        _require(fields[0] not in rows, f"passwd name is duplicated: {fields[0]}")
        rows[fields[0]] = fields
    return rows


def _group_rows(raw: bytes) -> dict[str, list[str]]:
    rows: dict[str, list[str]] = {}
    for line in raw.decode("utf-8").splitlines():
        if not line.strip():
            continue
        fields = line.split(":")
        _require(len(fields) == 4, f"group line does not have four fields: {line}")
        _require(fields[0] not in rows, f"group name is duplicated: {fields[0]}")
        rows[fields[0]] = fields
    return rows


def _verify_account_database(
    plan: dict[str, Any], source_lock: dict[str, Any]
) -> list[dict[str, Any]]:
    """Re-derive every clause the guest's identity resolver enforces.

    The plan lists the clauses. Listing them is not evidence, so the passwd and
    group bytes the lock tracks are parsed here and each clause is answered from
    them.
    """

    clauses = [row["clause"] for row in plan["identityContractClauses"]]
    _require(len(clauses) == len(set(clauses)), "the identity clause list repeats a clause")
    tracked = {row["logicalPath"]: row for row in source_lock["trackedFiles"]}
    for path in ("/etc/group", "/etc/gshadow", "/etc/nsswitch.conf", "/etc/passwd", "/etc/shadow"):
        _require(path in tracked, f"the successor lock does not track the account database: {path}")
    for path in _PASSWORD_BEARING:
        _require(
            tracked[path]["mode"] == _ROOT_ONLY_MODE,
            f"a password-bearing file is readable below root: {path}",
        )

    passwd = _passwd_rows((REPO_ROOT / tracked["/etc/passwd"]["sourcePath"]).read_bytes())
    group = _group_rows((REPO_ROOT / tracked["/etc/group"]["sourcePath"]).read_bytes())
    by_gid: dict[str, list[str]] = {}
    for name, fields in group.items():
        _require(fields[2] not in by_gid, f"two groups share a gid: {name}")
        by_gid[fields[2]] = fields

    accounts: list[dict[str, Any]] = []
    for name in FIXED_ACCOUNTS:
        fields = passwd.get(name)
        _require(fields is not None, f"the account database has no such account: {name}")
        uid, gid, home, shell = fields[2], fields[3], fields[5], fields[6]
        _require(uid.isdigit() and int(uid) != 0, f"account uid is zero or not a number: {name}")
        _require(gid.isdigit() and int(gid) != 0, f"account gid is zero or not a number: {name}")
        _require(home == REQUIRED_HOME, f"account home is not {REQUIRED_HOME}: {name}")
        _require(shell in ALLOWED_SHELLS, f"account shell is a real shell: {name}")
        primary = group.get(name)
        _require(primary is not None, f"the account has no same-named group: {name}")
        _require(primary[2] == gid, f"the same-named group is not at the account gid: {name}")
        _require(
            by_gid.get(gid, [None])[0] == name,
            f"a reverse lookup of the account gid does not return its group: {name}",
        )
        for group_name, group_fields in group.items():
            members = [member for member in group_fields[3].split(",") if member]
            _require(
                name not in members or group_name == name,
                f"the account is a member of a supplementary group: {name}",
            )
        _require(
            not [member for member in primary[3].split(",") if member],
            f"the account's own group lists members: {name}",
        )
        accounts.append({"gid": int(gid), "name": name, "uid": int(uid)})

    _require(
        len({row["uid"] for row in accounts}) == len(accounts),
        "the two fixed accounts share a uid",
    )
    _require(
        len({row["gid"] for row in accounts}) == len(accounts),
        "the two fixed accounts share a gid",
    )
    return sorted(accounts, key=lambda row: row["name"])


def _arm64_constant(name: str) -> str:
    """Read an arm64-gated launcher constant out of the sealed source.

    Both a gated and an ungated declaration exist for every one of these names,
    and the ungated one carries a different value, so the preceding attribute is
    read rather than the first matching line taken.
    """

    lines = AUTHORITY_ARCH_PATH.read_text(encoding="utf-8").splitlines()
    head = f"pub(crate) const {name}:"
    for index, line in enumerate(lines):
        if not line.startswith(head):
            continue
        gate = lines[index - 1]
        if 'feature = "linux-arm64-authority"' not in gate:
            continue
        if 'not(feature = "linux-arm64-authority")' in gate:
            continue
        cursor, text = index, line
        while not text.rstrip().endswith(";"):
            cursor += 1
            text += lines[cursor]
        value = text.split("=", 1)[1].strip().rstrip(";").strip()
        return value.strip('"')
    raise SourceLockError(f"the launcher has no arm64-gated constant named {name}")


def _arm64_number(name: str) -> int:
    """The same, for a numeric literal written with digit separators."""

    return int(_arm64_constant(name).replace("_", ""))


def _verify_nested_tree(plan: dict[str, Any], source_lock: dict[str, Any]) -> dict[str, Any]:
    """Check the declared nested tree against both places its digest is sealed."""

    trees = plan["nestedTrees"]
    _require(len(trees) == 1, "the plan declares more than one nested tree")
    tree = trees[0]
    _require(
        tree["state"] == "declared-not-assembled",
        "the plan claims the nested tree is already assembled",
    )
    _require(tree["requiresBuilderChange"] is True, "the plan claims the builder already stages it")

    manifest = tree["contentManifest"]
    _require(
        manifest["isATrackedSourceRow"] is False,
        "the content manifest is declared as a tracked source row",
    )
    tracked = {row["logicalPath"] for row in source_lock["trackedFiles"]}
    _require(
        manifest["guestPath"] not in tracked,
        "the successor lock tracks the content manifest as a source",
    )
    _require(
        not any(path.startswith(tree["guestPrefix"]) for path in tracked),
        "the successor lock tracks a file inside the nested tree",
    )
    _require(
        _arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SHA256") == manifest["sha256"],
        "the manifest digest differs from the value the launcher compiles against",
    )
    _require(
        _arm64_number("RUNTIME_ROOTFS_CONTENT_MANIFEST_SIZE") == manifest["sizeBytes"],
        "the manifest size differs from the value the launcher compiles against",
    )
    _require(
        _arm64_constant("RUNTIME_ROOTFS_CONTENT_MANIFEST_SCHEMA") == manifest["schema"],
        "the manifest schema differs from the value the launcher compiles against",
    )
    expectation = _load_json(EXPECTATION_PATH, "runtime rootfs replay expectation")
    _require(
        expectation["expectedOutput"]["rootfsContentManifestSha256"] == manifest["sha256"],
        "the manifest digest differs from the digest the replay expectation seals",
    )

    driver = tree["drivenBy"]
    driving = REPO_ROOT / driver["path"]
    _require(driving.is_file(), "the lock that drives the nested tree is absent")
    raw = driving.read_bytes()
    _require(
        sha256_bytes(raw) == driver["sha256"] and len(raw) == driver["sizeBytes"],
        "the lock that drives the nested tree differs from its sealed digest",
    )
    _require(
        driving.resolve() == BASELINE_LOCK_PATH.resolve(),
        "the nested tree is driven by a lock other than the sealed runtime lock",
    )
    runtime_lock = json.loads(raw.decode("utf-8"))
    _require(
        len(runtime_lock["artifacts"]) == driver["artifactCount"],
        "the driving lock's artifact count differs from the plan",
    )
    _require(
        len(runtime_lock["closureRoots"]) == driver["closureRootCount"],
        "the driving lock's closure root count differs from the plan",
    )
    return {
        "assembled": False,
        "contentManifestSha256": manifest["sha256"],
        "drivingSourceLockSha256": driver["sha256"],
        "guestPrefix": tree["guestPrefix"],
        "id": tree["id"],
        "layerSizeBytes": tree["layerSizeBytes"],
        "state": tree["state"],
    }


def _successor_requirements(plan: dict[str, Any]) -> list[dict[str, Any]]:
    """The contract's requirement rows with the recorded supersessions applied.

    Every row the frozen contract requires is kept. Two of them get the digest
    the plan records as their successor, and the account database rows the
    contract never had are added. Nothing is dropped and no digest becomes
    ``null``, so this list is stricter than the one it grows out of.
    """

    contract = _load_json(CONTRACT_PATH, "guest-init compatibility contract")
    moved = {row["logicalPath"]: row for row in plan["changesFromPredecessor"]["supersessions"]}
    rows: list[dict[str, Any]] = []
    for want in contract["trackedFileRequirements"]:
        row = dict(want)
        supersession = moved.get(row["logicalPath"])
        if supersession is not None:
            row["sha256"] = supersession["newSha256"]
        rows.append(row)
    known = {row["logicalPath"] for row in rows}
    for tracked in plan["trackedFiles"]:
        if tracked["logicalPath"] in known or not tracked["logicalPath"].startswith("/etc/"):
            continue
        rows.append(
            {
                "gid": tracked["gid"],
                "logicalPath": tracked["logicalPath"],
                "mode": tracked["mode"],
                "role": tracked["role"],
                "sha256": tracked["sha256"],
                "uid": tracked["uid"],
            }
        )
    return sorted(rows, key=lambda row: row["logicalPath"])


def _missing_roles(plan: dict[str, Any], source_lock: dict[str, Any]) -> list[str]:
    tracked = {row["logicalPath"]: row for row in source_lock["trackedFiles"]}
    missing: list[str] = []
    for want in _successor_requirements(plan):
        row = tracked.get(want["logicalPath"])
        if row is None:
            missing.append(f"tracked-file:{want['role']}")
            continue
        _require(
            row["mode"] == want["mode"],
            f"tracked file mode differs from the requirement: {want['role']}",
        )
        _require(
            row["uid"] == want["uid"] and row["gid"] == want["gid"],
            f"tracked file ownership differs from the requirement: {want['role']}",
        )
        _require(
            want["sha256"] is None or row["sha256"] == want["sha256"],
            f"tracked file digest differs from the requirement: {want['role']}",
        )
    return sorted(missing)


def _audit_shadow(plan: dict[str, Any], source_lock: dict[str, Any]) -> dict[str, Any]:
    """Ask the frozen contract its own question about everything that did not move."""

    shadow = build_shadow_lock(plan, source_lock)
    differing = [
        row["logicalPath"]
        for row, was in zip(source_lock["trackedFiles"], shadow["trackedFiles"])
        if row != was
    ]
    _require(
        differing == sorted(row["logicalPath"] for row in plan["changesFromPredecessor"]["supersessions"]),
        f"the successor differs from its shadow outside the recorded supersessions: {differing}",
    )
    with tempfile.TemporaryDirectory() as scratch:
        candidate = pathlib.Path(scratch) / "shadow-source-lock.json"
        candidate.write_bytes(canonical_json(shadow))
        try:
            audit = guest_init.audit_successor_source_shape(CONTRACT_PATH, candidate)
        except guest_init.GuestInitCompatibilityError as exc:
            raise SourceLockError(
                f"the frozen guest-init contract refused the unmoved part of the successor: {exc}"
            ) from exc
    sealed = _load_json(PREDECESSOR_RESULT_PATH, "predecessor boot source lock result")
    _require(
        audit["status"] == sealed["sourceShapeAudit"]["status"],
        "the frozen contract's verdict on the unmoved part differs from the predecessor's",
    )
    _require(
        audit["missingRoles"] == sealed["sourceShapeAudit"]["missingRoles"],
        "the frozen contract's missing roles on the unmoved part differ from the predecessor's",
    )
    return {
        "contractSha256": sha256_file(CONTRACT_PATH),
        "missingRoles": audit["missingRoles"],
        "note": (
            "the frozen contract pins the digest of both superseded files, so it refuses the "
            "successor lock itself. This verdict is its verdict on the successor with those two "
            "sources restored, which is what makes the supersessions the only difference"
        ),
        "shadowSourceLockSha256": audit["sourceLockSha256"],
        "status": audit["status"],
    }


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
        "accountDatabase": {
            "accounts": audit["accountsVerified"],
            "bakedIntoTheImage": True,
            "identityClausesVerified": audit["identityClausesVerified"],
            "provisionedAtBoot": False,
        },
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
            "nestedRuntimeTreeAssembled": False,
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
                "cause": row["closedBy"],
                "guestPath": predecessor.LAUNCHER_BINARY_GUEST_PATH,
                "resolvedBy": "arm64-launcher-build-authority",
                "role": row["role"],
            }
            for row in plan["guestInitRoles"]
            if row["state"] == "deferred"
        ],
        "gapsAddressed": sorted(row["id"] for row in plan["gapsAddressed"]),
        "generatorSha256": sha256_file(TOOL_PATH),
        "nestedTree": audit["nestedTree"],
        "payloadAcquisitionResultSha256": sha256_file(ACQUISITION_RESULT_PATH),
        "payloadsVerified": acquisition["boundaries"]["packagePayloadsVerified"],
        "planSha256": PLAN_SHA256,
        "predecessorBootSourceLockSha256": sha256_file(PREDECESSOR_LOCK_PATH),
        "predecessorContractAudit": audit["predecessorContractAudit"],
        "predecessorSourceLockSha256": sha256_file(BASELINE_LOCK_PATH),
        "productionByteProvenanceComplete": False,
        "schema": RESULT_SCHEMA,
        "signedRepositoryMetadataVerified": acquisition["signedRepositoryMetadataVerified"],
        "sourceLockSha256": audit["sourceLockSha256"],
        "sourceShapeAudit": {
            "missingRoles": audit["missingRoles"],
            "status": audit["status"],
        },
        "status": RESULT_STATUS,
        "supersessions": audit["supersessionsVerified"],
    }


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------
def build_and_verify() -> tuple[dict[str, Any], dict[str, Any], dict[str, Any]]:
    plan = load_plan()
    raw = PLAN_PATH.read_bytes()
    _require(
        sha256_bytes(raw) == PLAN_SHA256 and len(raw) == PLAN_SIZE_BYTES,
        "this tool pins a different plan than the plan on disk",
    )
    source_lock = build_source_lock(plan)
    audit = verify_source_lock(plan, source_lock)
    return plan, source_lock, build_result(plan, source_lock, audit)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument(
        "--dry-run",
        action="store_true",
        help="build and verify the successor documents without writing them",
    )
    group.add_argument("--check", action="store_true", help="verify the sealed documents")
    group.add_argument("--write", action="store_true", help="write the sealed documents")
    args = parser.parse_args(argv)

    _, source_lock, result = build_and_verify()
    documents = ((LOCK_PATH, source_lock), (RESULT_PATH, result))

    if args.dry_run:
        print(
            f"successor source lock: {result['sourceLockSha256']} "
            f"status={result['sourceShapeAudit']['status']} "
            f"missingRoles={result['sourceShapeAudit']['missingRoles']} sealed=no"
        )
        return 0

    if args.write:
        for path, document in documents:
            path.write_bytes(canonical_json(document))
        print(f"successor source lock written: {LOCK_PATH.name} ({result['sourceLockSha256']})")
        return 0

    for path, _ in documents:
        _require(
            path.is_file(),
            "the successor documents are not sealed yet. Sealing them is the third step of the "
            f"successor chain, not this one: {path.name}",
        )
    for path, document in documents:
        _require(
            path.read_bytes() == canonical_json(document),
            f"sealed document differs from the regenerated document: {path.name}",
        )
    print(
        f"successor source lock: {result['sourceLockSha256']} "
        f"status={result['sourceShapeAudit']['status']} "
        f"missingRoles={result['sourceShapeAudit']['missingRoles']}"
    )
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except SourceLockError as error:
        print(f"successor source lock refused: {error}", file=sys.stderr)
        sys.exit(1)
