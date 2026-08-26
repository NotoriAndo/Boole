#!/usr/bin/env python3
"""Frozen producer authority for the Linux/arm64 guest images (v2).

The v1 image builder authority (2026-08-26j) froze the outputs, the ext4
determinism knobs, the two tool binaries and the prohibitions, and it left two
things deliberately open: `determinism.sourceDateEpoch` was null, and the zstd
host tool carried `pinnedAtBuildTime: true` with a null digest.  It also could
not say where the build would run, because at the time there was no producer.

There is one now -- an arm64 Linux CI job -- so this successor states the rest:
which phase may reach the network and which structurally cannot, how the guest
launcher is obtained, what happens to the images afterwards, and the conditions
under which the run stops instead of lowering a bar.  The v1 document is left
exactly as sealed; a test asserts its digest.

Two choices here are worth stating plainly.

First, the pinned tool digests are not restated.  This document pins the v1
authority by digest and the producer reads mke2fs and debugfs out of it.  A
second copy could drift from the sealed one, and a drift between two copies of
the same fact is invisible.  Where a value genuinely is repeated -- the output
roles, the mismatch action, the canonical mtime -- the repetition is checked
against v1 mechanically rather than trusted.

Second, "no network during the build" is not written as a promise.  The acquire
phase runs with the network; the produce phase runs inside a transient systemd
unit with `PrivateNetwork=yes`, so it has no network namespace and a download
cannot fail open -- there is nothing to reach.  The same shape is already in use
in `scripts/native-shadow-portable-rootfs-replay-linux-arm64.sh`.

This module freezes a contract.  It produces no initrd, no root disk and no
boot, and every boundary it declares stays false.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import sys
from typing import Any, Optional


FORMAT_KEY = "format"
FORMAT = "initrd-ext4-producer-authority-v2"
SCHEMA = "boole.native-shadow.boot-image-producer-authority.arm64.v2"
RELEASE = "NATIVE-SHADOW-BOOT-IMAGE-PRODUCER-AUTHORITY-ARM64-V2"
RESULT_STATUS = "IMAGE-PRODUCER-AUTHORITY-FROZEN-NOTHING-PRODUCED"

AUTHORITY_PATH = (
    "native/containment/native-shadow-boot-image-producer-authority-arm64-v2.json"
)
GENERATOR_PATH = "scripts/native_shadow_boot_image_producer_authority_arm64_v2.py"

INHERITED_AUTHORITY_PATH = (
    "native/containment/native-shadow-boot-image-builder-authority-arm64-v1.json"
)
INHERITED_FORMAT_KEY = "format"
INHERITED_FORMAT = "initrd-ext4-builder-authority-v1"

PLAN_PATH = "native/containment/native-shadow-boot-artifact-build-plan-arm64-v2.json"
KERNEL_RESULT_PATH = (
    "native/containment/native-shadow-boot-kernel-extract-result-arm64-v1.json"
)
LAUNCHER_RESULT_PATH = (
    "native/containment/native-shadow-launcher-build-result-arm64-v1.json"
)
LAUNCHER_BUILD_SCRIPT = "scripts/native_shadow_launcher_build_arm64_v1.py"

AUTHORITY_KEYS = frozenset(
    {
        "abortConditions",
        "activationAllowed",
        "bootableClaim",
        "boundaries",
        "buildIsolation",
        "builderAuthority",
        "determinism",
        "format",
        "generator",
        "inputs",
        "launcher",
        "outputs",
        "platform",
        "release",
        "resultRetention",
        "schema",
    }
)

BOUNDARIES = {
    "bootAuthority": False,
    "guestBootVerified": False,
    "guestImageBuilt": False,
    "initrdBuilt": False,
    "launcherDeployedIntoGuest": False,
    "rootDiskBuilt": False,
    "runtimeCompatibilityVerified": False,
    "toolByteProvenanceVerifiedInCi": False,
}

# Every condition the operator named, plus the two the build itself can detect.
# `relaxKnobAllowed` is false on all of them: none of these is ever answered by
# turning a determinism knob down until the two results agree.
ABORT_CONDITIONS = [
    {
        "action": "stop-and-report-both-digests",
        "detects": "the two independent CI jobs produced different bytes",
        "id": "independent-builds-differ",
        "relaxKnobAllowed": False,
    },
    {
        "action": "stop-and-report-the-member",
        "detects": "a tool extracted from the frozen package failed its sha-256",
        "id": "tool-binary-digest-mismatch",
        "relaxKnobAllowed": False,
    },
    {
        "action": "stop-and-report-the-attempt",
        "detects": "the produce phase attempted to reach the network",
        "id": "network-attempted-during-produce",
        "relaxKnobAllowed": False,
    },
    {
        "action": "stop-and-report-the-package",
        "detects": "a maintainer script appeared in the consumed set",
        "id": "maintainer-script-present",
        "relaxKnobAllowed": False,
    },
    {
        "action": "stop-and-report-the-output",
        "detects": "a required output is missing or zero bytes",
        "id": "output-missing-or-empty",
        "relaxKnobAllowed": False,
    },
    {
        "action": "stop-and-report-both-digests",
        "detects": "the rebuilt launcher differs from the sealed build result",
        "id": "launcher-digest-mismatch",
        "relaxKnobAllowed": False,
    },
]

# Copied from the shape already in use for the portable rootfs replay.  With no
# network namespace there is nothing for a build-time download to reach, so the
# prohibition is enforced by the kernel rather than by this document.
SYSTEMD_RUN_PROPERTIES = [
    "PrivateNetwork=yes",
    "ProtectSystem=strict",
    "NoNewPrivileges=yes",
    "PrivateDevices=yes",
    "PrivateMounts=yes",
    "RestrictAddressFamilies=AF_UNIX",
]

PLATFORM = {
    "architecture": "aarch64",
    "os": "linux",
    "rustTarget": "aarch64-unknown-linux-gnu",
}

RESULT_RETENTION = {
    "commitImagesToGit": False,
    "keep": "ci-artifact-and-sha256-manifest",
    "manifestFormat": "sha256sum-text",
    "uploadToRelease": False,
}

AUTHORITY_SHA256 = "2af2fddfba29ef3302274c8e8e9ddf0e06bacb977f2efd5ce78844b9a2b3b12a"

HEX = frozenset("0123456789abcdef")


class ProducerAuthorityError(RuntimeError):
    """The producer authority or one of its pinned inputs is invalid."""


def canonical_bytes(value: Any) -> bytes:
    return (
        json.dumps(
            value, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False
        ).encode("utf-8")
        + b"\n"
    )


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def generator_sha256(raw: bytes) -> str:
    """Digest this tool with its own authority pin blanked out.

    The document names this tool and this tool names the document, so a plain
    file digest can never equal the pin.  Blanking the literal breaks the cycle
    in a way both sides reproduce -- the same trick the v1 authority uses.
    """

    marker = b'AUTHORITY_SHA256 = "'
    start = raw.index(marker) + len(marker)
    return sha256_bytes(raw[:start] + b"0" * 64 + raw[start + 64 :])


def _digest(value: Any, name: str) -> str:
    if not isinstance(value, str) or len(value) != 64 or not set(value) <= HEX:
        raise ProducerAuthorityError(f"{name} is not a lowercase sha-256 digest")
    return value


def _size(value: Any, name: str) -> int:
    # bool is an int in Python; a size of True must not pass as 1.
    if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
        raise ProducerAuthorityError(f"{name} is not a positive byte count")
    return value


def _exact(value: Any, keys: frozenset[str], name: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ProducerAuthorityError(f"{name} is not an object")
    if set(value) != set(keys):
        missing = sorted(set(keys) - set(value))
        extra = sorted(set(value) - set(keys))
        raise ProducerAuthorityError(
            f"{name} keys differ (missing={missing}, unexpected={extra})"
        )
    return value


def _tracked(root: pathlib.Path, relative: str) -> bytes:
    path = root / relative
    if not path.is_file():
        raise ProducerAuthorityError(f"tracked file is missing: {relative}")
    return path.read_bytes()


def _pin(value: Any, name: str) -> dict[str, Any]:
    pin = _exact(value, frozenset({"path", "sha256", "sizeBytes"}), name)
    _digest(pin["sha256"], f"{name}.sha256")
    _size(pin["sizeBytes"], f"{name}.sizeBytes")
    return pin


def check_pin(root: pathlib.Path, pin: dict[str, Any], name: str) -> None:
    raw = _tracked(root, pin["path"])
    if sha256_bytes(raw) != pin["sha256"]:
        raise ProducerAuthorityError(f"{name} does not match {pin['path']}")
    if len(raw) != pin["sizeBytes"]:
        raise ProducerAuthorityError(f"{name} size does not match {pin['path']}")


def load_authority(path: pathlib.Path) -> dict[str, Any]:
    raw = path.read_bytes()
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ProducerAuthorityError(f"authority is not valid json: {exc}") from exc
    if canonical_bytes(value) != raw:
        raise ProducerAuthorityError("authority is not canonical json")
    document = _exact(value, AUTHORITY_KEYS, "authority")
    if document["schema"] != SCHEMA:
        raise ProducerAuthorityError("authority schema differs")
    if document["release"] != RELEASE:
        raise ProducerAuthorityError("authority release differs")
    if document[FORMAT_KEY] != FORMAT:
        raise ProducerAuthorityError("authority format differs")
    if document["activationAllowed"] is not False:
        raise ProducerAuthorityError("authority must not allow activation")
    if document["bootableClaim"] is not False:
        raise ProducerAuthorityError("authority must not claim bootability")
    if document["boundaries"] != BOUNDARIES:
        raise ProducerAuthorityError("authority boundaries differ from the contract")
    if document["abortConditions"] != ABORT_CONDITIONS:
        raise ProducerAuthorityError("authority abort conditions differ")
    _pin(document["builderAuthority"], "builderAuthority")
    _pin(document["inputs"]["plan"], "inputs.plan")
    _pin(document["inputs"]["kernelResult"], "inputs.kernelResult")
    return document


def inherited_authority(root: pathlib.Path, document: dict[str, Any]) -> dict[str, Any]:
    """Load the sealed v1 authority named by this document, checking its digest."""

    pin = _pin(document["builderAuthority"], "builderAuthority")
    if pin["path"] != INHERITED_AUTHORITY_PATH:
        raise ProducerAuthorityError("builderAuthority names a different document")
    check_pin(root, pin, "builderAuthority")
    inherited = json.loads(_tracked(root, pin["path"]))
    # Explicit key, no fallback chain: the two earlier authority documents
    # disagreed on this field's name and a fallback hid the difference.
    if inherited.get(INHERITED_FORMAT_KEY) != INHERITED_FORMAT:
        raise ProducerAuthorityError(
            "builderAuthority document does not declare the v1 builder format"
        )
    return inherited


def check_inherited_authority(root: pathlib.Path, document: dict[str, Any]) -> None:
    """Every value repeated from v1 must still agree with v1."""

    inherited = inherited_authority(root, document)
    determinism = document["determinism"]
    if determinism["sourceDateEpoch"] != inherited["determinism"]["canonicalMtime"]:
        raise ProducerAuthorityError(
            "sourceDateEpoch disagrees with the sealed canonical mtime"
        )
    if inherited["determinism"]["sourceDateEpoch"] is not None:
        raise ProducerAuthorityError(
            "the sealed authority no longer leaves sourceDateEpoch open"
        )
    if determinism["mismatchAction"] != inherited["determinism"]["mismatchAction"]:
        raise ProducerAuthorityError("mismatchAction disagrees with the sealed value")
    if determinism["independentJobCount"] != inherited["determinism"][
        "independentBuildCount"
    ]:
        raise ProducerAuthorityError(
            "independentJobCount disagrees with the sealed build count"
        )
    if document["outputs"] != inherited["roles"]["outputs"]:
        raise ProducerAuthorityError("outputs disagree with the sealed roles")
    if document["platform"] != inherited["platform"]:
        raise ProducerAuthorityError("platform disagrees with the sealed value")
    for name, allowed in inherited["prohibitions"].items():
        if allowed is not True:
            raise ProducerAuthorityError(f"sealed prohibition {name} is no longer set")


def inherited_tool_binaries(
    root: pathlib.Path, document: dict[str, Any]
) -> list[dict[str, Any]]:
    """The pinned tools, read out of the sealed document rather than restated."""

    tools = inherited_authority(root, document)["toolBinaries"]
    if not isinstance(tools, list) or not tools:
        raise ProducerAuthorityError("the sealed authority pins no tool binaries")
    for tool in tools:
        _digest(tool.get("sha256"), f"tool {tool.get('role')} sha256")
        _size(tool.get("sizeBytes"), f"tool {tool.get('role')} sizeBytes")
    return tools


def abort_ids(document: dict[str, Any]) -> list[str]:
    return [condition["id"] for condition in document["abortConditions"]]


def check_launcher(root: pathlib.Path, document: dict[str, Any]) -> None:
    launcher = document["launcher"]
    sealed = json.loads(_tracked(root, launcher["resultPath"]))["launcher"]
    for field in ("sha256", "sizeBytes", "guestLogicalPath"):
        if launcher[field] != sealed[field]:
            raise ProducerAuthorityError(
                f"launcher {field} disagrees with the sealed build result"
            )
    check_pin(root, _pin(launcher["buildScript"], "launcher.buildScript"), "launcher.buildScript")


def check_generator(root: pathlib.Path, document: dict[str, Any]) -> None:
    generator = _pin(document["generator"], "generator")
    if generator["path"] != GENERATOR_PATH:
        raise ProducerAuthorityError("generator names a different tool")
    raw = _tracked(root, generator["path"])
    if generator_sha256(raw) != generator["sha256"]:
        raise ProducerAuthorityError("generator digest differs from the pin")
    if len(raw) != generator["sizeBytes"]:
        raise ProducerAuthorityError("generator size differs from the pin")


def verify(root: pathlib.Path) -> dict[str, Any]:
    document = load_authority(root / AUTHORITY_PATH)
    raw = _tracked(root, AUTHORITY_PATH)
    if sha256_bytes(raw) != AUTHORITY_SHA256:
        raise ProducerAuthorityError("producer authority differs from the pin")
    check_inherited_authority(root, document)
    check_generator(root, document)
    check_launcher(root, document)
    check_pin(root, document["inputs"]["plan"], "inputs.plan")
    check_pin(root, document["inputs"]["kernelResult"], "inputs.kernelResult")
    tools = inherited_tool_binaries(root, document)
    return {
        "abortConditions": len(document["abortConditions"]),
        "outputs": len(document["outputs"]),
        "status": RESULT_STATUS,
        "toolBinaries": len(tools),
    }


def build_document(root: pathlib.Path) -> dict[str, Any]:
    """Assemble the document from the tracked files it pins."""

    def pin(relative: str) -> dict[str, Any]:
        raw = _tracked(root, relative)
        return {
            "path": relative,
            "sha256": sha256_bytes(raw),
            "sizeBytes": len(raw),
        }

    inherited = json.loads(_tracked(root, INHERITED_AUTHORITY_PATH))
    sealed_launcher = json.loads(_tracked(root, LAUNCHER_RESULT_PATH))["launcher"]
    generator_raw = _tracked(root, GENERATOR_PATH)
    return {
        "abortConditions": ABORT_CONDITIONS,
        "activationAllowed": False,
        "bootableClaim": False,
        "boundaries": dict(BOUNDARIES),
        "buildIsolation": {
            "phases": [
                {
                    "name": "acquire",
                    "networkAllowed": True,
                    "reads": "pinned urls with pinned digests into a scratch store",
                },
                {
                    "name": "produce",
                    "networkAllowed": False,
                    "reads": "only the scratch store and the read-only repository",
                },
            ],
            "runner": "ubuntu-24.04-arm",
            "separateJobs": True,
            "systemdRunProperties": list(SYSTEMD_RUN_PROPERTIES),
        },
        "builderAuthority": pin(INHERITED_AUTHORITY_PATH),
        "determinism": {
            "hostToolPinning": "record-at-build-time",
            "independentJobCount": inherited["determinism"]["independentBuildCount"],
            "inheritedFrom": INHERITED_AUTHORITY_PATH,
            "mismatchAction": inherited["determinism"]["mismatchAction"],
            "sourceDateEpoch": inherited["determinism"]["canonicalMtime"],
        },
        "format": FORMAT,
        "generator": {
            "path": GENERATOR_PATH,
            "sha256": generator_sha256(generator_raw),
            "sizeBytes": len(generator_raw),
        },
        "inputs": {
            "kernelResult": pin(KERNEL_RESULT_PATH),
            "plan": pin(PLAN_PATH),
        },
        "launcher": {
            "acquisition": "rebuild-and-match-seal",
            "buildScript": pin(LAUNCHER_BUILD_SCRIPT),
            "guestLogicalPath": sealed_launcher["guestLogicalPath"],
            "resultPath": LAUNCHER_RESULT_PATH,
            "sha256": sealed_launcher["sha256"],
            "sizeBytes": sealed_launcher["sizeBytes"],
        },
        "outputs": inherited["roles"]["outputs"],
        "platform": inherited["platform"],
        "release": RELEASE,
        "resultRetention": dict(RESULT_RETENTION),
        "schema": SCHEMA,
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    for name in ("check", "emit"):
        sub = subparsers.add_parser(name)
        sub.add_argument("--root", default=".")
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    root = pathlib.Path(args.root).resolve()
    try:
        if args.command == "emit":
            document = build_document(root)
            (root / AUTHORITY_PATH).write_bytes(canonical_bytes(document))
            raw = _tracked(root, AUTHORITY_PATH)
            print(f"wrote {AUTHORITY_PATH}")
            print(f"sha256 {sha256_bytes(raw)}  size {len(raw)}")
            return 0
        report = verify(root)
        print(
            "image producer authority: "
            f"outputs={report['outputs']} tools={report['toolBinaries']} "
            f"aborts={report['abortConditions']} built=no"
        )
        print(f"status: {report['status']}")
        return 0
    except ProducerAuthorityError as exc:
        print(f"image producer authority refused: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
