#!/usr/bin/env python3
"""Successor boot artifact build plan for Linux/arm64 (v2).

The audit-only v1 preflight refuses any plan whose three authority slots carry
a digest:

    inputs.<name> belongs to an audit-only scaffold and must stay null;
    use a successor plan/schema/tool

All three authorities now exist -- the image builder (2026-08-26j), the guest
kernel (2026-08-26l) and the systemd guest closure (2026-08-26m) -- so this is
that successor.  Its rules are the mirror image of v1's: here a null slot is the
error.  The v1 scaffold is left exactly as it is; a test asserts that.

Two of the three slots pin an authority DOCUMENT that declares its own format,
and one pins raw IMAGE bytes and has no document to ask.  Handling all three
through one code path would silently accept the wrong object in at least one
slot, so the two kinds are separated here.

This slice resolves inputs.  It builds nothing, boots nothing and grants no
authority to boot.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
from typing import Any, Optional


RELEASE = "NATIVE-SHADOW-BOOT-ARTIFACT-BUILD-PLAN-ARM64-V2-RESOLVED-NOT-ACTIVATABLE"
PLAN_SCHEMA = "boole.native-shadow.boot-artifact-build-plan.arm64.v2"
SCAFFOLD_SCHEMA = "boole.native-shadow.boot-artifact-build-plan.arm64.v1"
RESULT_STATUS = "BOOT-INPUT-AUTHORITIES-RESOLVED-NOT-BOOT-AUTHORITY"

PLAN_PATH = "native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json"
SCAFFOLD_PATH = (
    "native/containment/native-shadow-boot-artifact-build-plan-arm64-v1-scaffold.json"
)
KERNEL_RESULT_PATH = (
    "native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json"
)

SLOT_FORMATS = {
    "imageBuilderToolchain": "initrd-ext4-builder-authority-v1",
    "kernel": "linux-arm64-image",
    "systemdGuestClosure": "systemd-rootfs-closure-authority-v1",
}

# Slots whose pin names an authority document.  Each of these documents states
# the same format string in its own `format` field, so the slot can be checked
# against the object rather than against a name we chose.
DOCUMENT_SLOTS = ("imageBuilderToolchain", "systemdGuestClosure")

# The two documents do NOT agree on what to call that field: the image builder
# authority says `format`, the systemd closure result says `closureFormat`.  A
# reader that tried `format` and fell back to the other name would accept either
# document in either slot, so the key is pinned per slot instead.
SLOT_FORMAT_KEYS = {
    "imageBuilderToolchain": "format",
    "systemdGuestClosure": "closureFormat",
}
SLOT_DOCUMENTS = {
    "imageBuilderToolchain": (
        "native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json"
    ),
    "systemdGuestClosure": (
        "native/containment/native-shadow-boot-systemd-closure-result-arm64-v1.json"
    ),
}

# Slots whose pin names raw artifact bytes.  The kernel result document has no
# `format` field at all, so it is not what this slot pins -- the slot pins the
# bytes that document describes, and the document is where the pin is checked.
ARTIFACT_SLOTS = ("kernel",)

PIN_KEYS = frozenset({"format", "sha256", "sizeBytes"})
PLAN_KEYS = frozenset(
    {
        "activationAllowed",
        "bootFormatVersion",
        "bootableClaim",
        "guestDownloadMaxBytes",
        "guestExecutionPolicy",
        "inputs",
        "release",
        "rootfsSourceLock",
        "schema",
    }
)

ACTIVATION_ALLOWED = False
BOUNDARIES = {
    "bootAuthority": False,
    "bootInputAuthoritiesResolved": True,
    "guestBootVerified": False,
    "guestImageBuilt": False,
    "initrdBuilt": False,
    "launcherDeployedIntoGuest": False,
    "rootDiskBuilt": False,
    "runtimeCompatibilityVerified": False,
}

HEX = frozenset("0123456789abcdef")


class BootArtifactPlanError(RuntimeError):
    """Raised when a resolved plan does not hold up."""


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(
            value, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False
        ).encode("utf-8")
        + b"\n"
    )


def _digest(value: Any, name: str) -> str:
    if not isinstance(value, str) or len(value) != 64 or not set(value) <= HEX:
        raise BootArtifactPlanError(f"{name} is not a lowercase sha-256 digest")
    return value


def _size(value: Any, name: str) -> int:
    # bool is an int in Python; a size of True must not pass as 1.
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise BootArtifactPlanError(f"{name} is not a positive byte count")
    return value


def _exact(value: Any, keys: frozenset[str], name: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise BootArtifactPlanError(f"{name} is not an object")
    if set(value) != set(keys):
        missing = sorted(set(keys) - set(value))
        extra = sorted(set(value) - set(keys))
        raise BootArtifactPlanError(
            f"{name} keys differ (missing={missing}, unexpected={extra})"
        )
    return value


def resolved_pin(value: Any, slot: str) -> dict[str, Any]:
    """Validate one slot's pin.  Unlike v1, a null here is the error."""
    pin = _exact(value, PIN_KEYS, f"inputs.{slot}")
    expected = SLOT_FORMATS.get(slot)
    if expected is None:
        raise BootArtifactPlanError(f"inputs.{slot} is not a known slot")
    if pin["format"] != expected:
        raise BootArtifactPlanError(
            f"inputs.{slot}.format is {pin['format']!r}, expected {expected!r}"
        )
    _digest(pin["sha256"], f"inputs.{slot}.sha256")
    _size(pin["sizeBytes"], f"inputs.{slot}.sizeBytes")
    return pin


def check_document_format(slot: str, document: Any) -> None:
    """A document must declare the slot's format in its own bytes.

    A document whose digest was pinned correctly but which declares a different
    format is the wrong object sitting in a right-looking slot.
    """
    if not isinstance(document, dict):
        raise BootArtifactPlanError(f"inputs.{slot} document is not an object")
    key = SLOT_FORMAT_KEYS[slot]
    declared = document.get(key)
    if declared is None:
        raise BootArtifactPlanError(
            f"inputs.{slot} document declares no {key}; it cannot fill a "
            "document slot"
        )
    expected = SLOT_FORMATS[slot]
    if declared != expected:
        raise BootArtifactPlanError(
            f"inputs.{slot} document declares format {declared!r}, "
            f"expected {expected!r}"
        )


def load_plan(path: pathlib.Path) -> dict[str, Any]:
    raw = path.read_bytes()
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise BootArtifactPlanError(f"plan is not valid json: {exc}") from exc
    if canonical_bytes(value) != raw:
        raise BootArtifactPlanError("plan is not canonical json")
    plan = _exact(value, PLAN_KEYS, "plan")
    if plan["schema"] != PLAN_SCHEMA:
        raise BootArtifactPlanError("plan schema differs")
    if plan["release"] != RELEASE:
        raise BootArtifactPlanError("plan release differs")
    if plan["activationAllowed"] is not False:
        raise BootArtifactPlanError("plan must not allow activation")
    if plan["bootableClaim"] is not False:
        raise BootArtifactPlanError("plan must not claim bootability")
    inputs = _exact(plan["inputs"], frozenset(SLOT_FORMATS), "inputs")
    for slot in sorted(inputs):
        resolved_pin(inputs[slot], slot)
    return plan


def _tracked(root: pathlib.Path, relative: str) -> bytes:
    path = root / relative
    if not path.is_file():
        raise BootArtifactPlanError(f"tracked document is missing: {relative}")
    return path.read_bytes()


def verify_slots(root: pathlib.Path, plan: dict[str, Any]) -> dict[str, Any]:
    """Check every slot's pin against a document tracked in this repository.

    All three slots are reproducible on a clean runner.  The kernel image bytes
    are not in the repository, but the sealed extraction result is, so the pin's
    agreement with it is checkable here.  Whether those bytes exist anywhere is
    a different fact and is not claimed by this function.
    """
    report: dict[str, Any] = {}
    for slot in sorted(SLOT_FORMATS):
        pin = resolved_pin(plan["inputs"][slot], slot)
        if slot in DOCUMENT_SLOTS:
            relative = SLOT_DOCUMENTS[slot]
            raw = _tracked(root, relative)
            document = json.loads(raw)
            check_document_format(slot, document)
            actual_digest = hashlib.sha256(raw).hexdigest()
            actual_size = len(raw)
        else:
            relative = KERNEL_RESULT_PATH
            sealed = json.loads(_tracked(root, relative))
            kernel = sealed.get("kernel")
            if not isinstance(kernel, dict):
                raise BootArtifactPlanError("sealed kernel result has no kernel block")
            actual_digest = _digest(kernel.get("sha256"), "sealed kernel sha256")
            actual_size = _size(kernel.get("sizeBytes"), "sealed kernel sizeBytes")
        if actual_digest != pin["sha256"]:
            raise BootArtifactPlanError(
                f"inputs.{slot}.sha256 does not match {relative}"
            )
        if actual_size != pin["sizeBytes"]:
            raise BootArtifactPlanError(
                f"inputs.{slot}.sizeBytes does not match {relative}"
            )
        report[slot] = {
            "checkedAgainst": relative,
            "digestMatches": True,
            "format": pin["format"],
            "kind": "document" if slot in DOCUMENT_SLOTS else "artifact",
            "reproducibleInCi": True,
            "sizeMatches": True,
        }
    return report


def check_scaffold_untouched(root: pathlib.Path) -> None:
    """The successor exists so the scaffold never has to be edited."""
    scaffold = json.loads(_tracked(root, SCAFFOLD_PATH))
    if scaffold.get("schema") != SCAFFOLD_SCHEMA:
        raise BootArtifactPlanError("v1 scaffold schema differs")
    for slot in sorted(SLOT_FORMATS):
        pin = scaffold.get("inputs", {}).get(slot, {})
        if pin.get("sha256") is not None or pin.get("sizeBytes") is not None:
            raise BootArtifactPlanError(
                f"v1 scaffold slot {slot} was filled in place; the v1 preflight "
                "will now reject it"
            )


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    check = subparsers.add_parser("check")
    check.add_argument("--root", default=".")
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    root = pathlib.Path(args.root).resolve()
    try:
        check_scaffold_untouched(root)
        plan = load_plan(root / PLAN_PATH)
        report = verify_slots(root, plan)
    except BootArtifactPlanError as exc:
        print(f"boot artifact plan v2: {exc}", file=sys.stderr)
        return 1
    slots = ", ".join(f"{k}={v['kind']}" for k, v in sorted(report.items()))
    print(f"boot artifact plan v2: resolved {slots}")
    print(f"status: {RESULT_STATUS}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
