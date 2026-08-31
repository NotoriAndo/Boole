#!/usr/bin/env python3
"""Reversible Mac readiness runner for disposable closed-local guest images.

This is deliberately separate from the historical one-use qualification
drivers.  It accepts only a byte-identical development comparison receipt,
recomputes every image digest before and after the run, and turns the four
launcher-v2 console observations into a development-readiness result.  It
cannot grant production, activation, reward, testnet, or public-mining status.
"""

import argparse
import hashlib
import json
import os
import pathlib
import platform
import subprocess
import sys


REPO = pathlib.Path(__file__).resolve().parents[1]
if str(REPO) not in sys.path:
    sys.path.insert(0, str(REPO))

from scripts import native_shadow_mac3_guest_evidence_protocol_arm64_v2 as protocol


IMAGE_NAMES = ("guest-kernel", "guest-initrd", "guest-root-disk")
COMPARISON_SCHEMA = (
    "boole.native-shadow.closed-local-image-replica-comparison.arm64.v1"
)
RESULT_SCHEMA = "boole.native-shadow.closed-local-mac-readiness.arm64.v1"
HOST_SOURCE = REPO / "native/mac3/boole-mac3-closed-local-boot.swift"
ENTITLEMENTS = REPO / "native/mac3/boole-mac3-closed-local-boot.entitlements"
LAUNCHER_BUILD_RESULT = (
    REPO / "native/containment/native-shadow-launcher-build-result-arm64-v2.json"
)
LAUNCHER_INTEGRATION = (
    REPO
    / "native/containment/native-shadow-mac3-launcher-v2-image-integration-preregistration-arm64-v1.json"
)
LAUNCHER_GUEST_PATH = "/usr/libexec/boole/boole-native-shadow-launcher"
KERNEL_COMMAND_LINE = "console=hvc0 root=/dev/vda ro init=/usr/lib/systemd/systemd"
EXACT_MACHINE = {
    "cpuCount": 2,
    "memoryBytes": 2 * 1024 * 1024 * 1024,
    "networkDevices": 0,
    "sharedDirectories": 0,
    "socketDevices": 0,
    "storageDevices": 1,
    "serialPorts": 1,
}


def _json(path):
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise ValueError("cannot read JSON %s: %s" % (path, error))
    if not isinstance(value, dict):
        raise ValueError("%s must contain one JSON object" % path)
    return value


def _sealed_launcher_identity():
    build = _json(LAUNCHER_BUILD_RESULT)
    integration = _json(LAUNCHER_INTEGRATION)
    launcher = build.get("launcher")
    staged = integration.get("launcherStaging") or integration.get("launcher")
    if not isinstance(launcher, dict):
        raise ValueError("the launcher-v2 build result has no launcher identity")
    if not isinstance(staged, dict):
        # The preregistration exposes the same values at a small, fixed row.
        candidates = []
        stack = [integration]
        while stack:
            value = stack.pop()
            if isinstance(value, dict):
                if (
                    value.get("guestLogicalPath") == LAUNCHER_GUEST_PATH
                    and "sha256" in value
                    and "sizeBytes" in value
                ):
                    candidates.append(value)
                stack.extend(value.values())
            elif isinstance(value, list):
                stack.extend(value)
        if not candidates:
            raise ValueError("the launcher-v2 staging record has no exact guest identity")
        staged = candidates[0]
    if launcher.get("sha256") != staged.get("sha256"):
        raise ValueError("launcher-v2 build and staging digests differ")
    if launcher.get("sizeBytes") != staged.get("sizeBytes"):
        raise ValueError("launcher-v2 build and staging sizes differ")
    return staged["sha256"], staged["sizeBytes"]


LAUNCHER_SHA256, LAUNCHER_SIZE_BYTES = _sealed_launcher_identity()


def _sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while True:
            block = handle.read(4 * 1024 * 1024)
            if not block:
                break
            digest.update(block)
    return digest.hexdigest()


def _exact_comparison(path):
    value = _json(path)
    if value.get("schema") != COMPARISON_SCHEMA:
        raise ValueError("comparison schema differs from %s" % COMPARISON_SCHEMA)
    if value.get("status") != "TWO-REPLICAS-BYTE-IDENTICAL":
        raise ValueError("comparison status is not byte-identical")
    if value.get("artifactClass") != "DISPOSABLE-DEVELOPMENT":
        raise ValueError("comparison artifactClass is not disposable development")
    for field in ("activationAllowed", "bootVerified", "productionRelease"):
        if value.get(field) is not False:
            raise ValueError("comparison %s must be false" % field)
    rows = value.get("outputs")
    if not isinstance(rows, list) or len(rows) != len(IMAGE_NAMES):
        raise ValueError("comparison must contain exactly three outputs")
    if [row.get("name") for row in rows if isinstance(row, dict)] != list(IMAGE_NAMES):
        raise ValueError("comparison output names or order differ")
    for row in rows:
        if set(row) != {"name", "sha256", "sizeBytes"}:
            raise ValueError("comparison output row has fields outside the exact schema")
        if (
            not isinstance(row["sha256"], str)
            or len(row["sha256"]) != 64
            or any(character not in "0123456789abcdef" for character in row["sha256"])
        ):
            raise ValueError("comparison output digest is malformed")
        if not isinstance(row["sizeBytes"], int) or isinstance(row["sizeBytes"], bool):
            raise ValueError("comparison output size is malformed")
    return value


def bind_images(comparison_path, images):
    comparison_path = pathlib.Path(comparison_path)
    comparison = _exact_comparison(comparison_path)
    if set(images) != set(IMAGE_NAMES):
        raise ValueError("image paths must name exactly %s" % ", ".join(IMAGE_NAMES))
    bound = []
    for sealed in comparison["outputs"]:
        name = sealed["name"]
        supplied = pathlib.Path(images[name])
        if supplied.is_symlink() or not supplied.is_file():
            raise ValueError("%s is not one regular image file" % name)
        path = supplied.resolve()
        found_size = path.stat().st_size
        if found_size != sealed["sizeBytes"]:
            raise ValueError("%s size differs from the comparison receipt" % name)
        found_sha256 = _sha256(path)
        if found_sha256 != sealed["sha256"]:
            raise ValueError("%s digest differs from the comparison receipt" % name)
        bound.append(
            {
                "name": name,
                "path": str(path),
                "sha256": found_sha256,
                "sizeBytes": found_size,
            }
        )
    return bound


def _host_receipt_matches(receipt, expected_images=None):
    if receipt.get("schema") != "boole.native-shadow.mac3-closed-local-boot-run.v1":
        return False, "the Mac host receipt schema differs"
    if receipt.get("dryRun") is not False:
        return False, "the Mac host receipt describes a dry run"
    if receipt.get("machine") != EXACT_MACHINE:
        return False, "the Mac VM devices or fixed resource shape differ"
    if receipt.get("rootDisk", {}).get("attachedReadOnly") is not True:
        return False, "the root disk was not attached read-only"
    if receipt.get("kernelCommandLine") not in (None, KERNEL_COMMAND_LINE):
        return False, "the Mac host kernel command line differs"
    if expected_images is not None:
        rows = {row["name"]: row for row in expected_images}
        if receipt.get("kernel", {}).get("sha256") != rows["guest-kernel"]["sha256"]:
            return False, "the Mac host receipt names a different kernel digest"
        if (
            receipt.get("rootDisk", {}).get("sha256")
            != rows["guest-root-disk"]["sha256"]
        ):
            return False, "the Mac host receipt names a different root-disk digest"
    if receipt.get("outcome") not in ("stopped-at-timeout", "guest-stopped"):
        return False, "the Mac host did not complete or stop the guest"
    return True, "the Mac host used the exact closed, read-only VM shape"


def assess_readiness(transcript, receipt, expected_images=None):
    read = protocol.read_transcript(transcript)
    checks = {
        "launcher-executable": protocol.launcher_executable_matches(
            read,
            expected_path=LAUNCHER_GUEST_PATH,
            expected_sha256=LAUNCHER_SHA256,
        ),
        "launcher-prerequisites": protocol.prerequisites_match(read),
        "supervisor-privilege": protocol.supervisor_matches(read),
        "readiness": protocol.readiness_matches(read),
    }
    evidence = {
        identifier: {"met": met, "detail": detail}
        for identifier, (met, detail) in checks.items()
    }
    host_met, host_detail = _host_receipt_matches(receipt, expected_images)
    return {
        "ready": host_met and all(row["met"] for row in evidence.values()),
        "hostIsolation": {"met": host_met, "detail": host_detail},
        "guestEvidence": evidence,
        "malformedGuestRecords": read["malformed"],
        "conflictingGuestRecords": read["conflicting"],
        "missingGuestRecords": read["missing"],
        "unknownGuestRecordIds": read["unknownRecordIds"],
        "submissionsObserved": False,
    }


def make_result(
    *, mode, images_before, images_after, host_receipt, assessment, transcript_sha256
):
    unchanged = images_before == images_after
    ready = mode == "boot" and unchanged and assessment.get("ready") is True
    status = {
        "preflight": "CLOSED-LOCAL-MAC-PREFLIGHT-PASS",
        "boot": (
            "CLOSED-LOCAL-MAC-READINESS-PASS"
            if ready
            else "CLOSED-LOCAL-MAC-READINESS-FAIL"
        ),
    }[mode]
    return {
        "schema": RESULT_SCHEMA,
        "status": status,
        "mode": mode,
        "artifactClass": "DISPOSABLE-DEVELOPMENT",
        "imagesBefore": images_before,
        "imagesAfter": images_after,
        "imagesUnchanged": unchanged,
        "hostReceipt": host_receipt,
        "assessment": assessment,
        "consoleTranscriptSha256": transcript_sha256,
        "directKernelBoot": True,
        "initrdBoundButNotUsedByDirectBoot": True,
        "activationAllowed": False,
        "productionRelease": False,
        "publicMining": False,
        "rewardReady": False,
        "testnetClaim": False,
        "nodeConnected": False,
        "consensusTouched": False,
        "p2pTouched": False,
    }


def _run(argv):
    subprocess.run(argv, check=True)


def swiftc_argv(swiftc, sdk, module_cache, binary):
    return [
        str(swiftc),
        "-sdk",
        str(sdk),
        "-target",
        "arm64-apple-macos14.0",
        "-module-cache-path",
        str(module_cache),
        "-O",
        "-framework",
        "Virtualization",
        str(HOST_SOURCE),
        "-o",
        str(binary),
    ]


def _host_argv(binary, bound, console, receipt, timeout, dry_run):
    rows = {row["name"]: row for row in bound}
    argv = [
        str(binary),
        "--kernel",
        rows["guest-kernel"]["path"],
        "--kernel-sha256",
        rows["guest-kernel"]["sha256"],
        "--root-disk",
        rows["guest-root-disk"]["path"],
        "--root-disk-sha256",
        rows["guest-root-disk"]["sha256"],
        "--cmdline",
        KERNEL_COMMAND_LINE,
        "--console",
        str(console),
        "--receipt",
        str(receipt),
        "--timeout",
        str(timeout),
    ]
    if dry_run:
        argv.append("--dry-run")
    return argv


def _write_json(path, value):
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    os.replace(str(temporary), str(path))


def execute(args):
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise ValueError("closed-local Mac readiness requires Apple Silicon macOS")
    if tuple(int(piece) for piece in platform.mac_ver()[0].split(".")[:1]) < (14,):
        raise ValueError("closed-local Mac readiness requires macOS 14 or newer")
    work = pathlib.Path(args.work).resolve()
    result_path = pathlib.Path(args.result).resolve()
    if work.exists() and any(work.iterdir()):
        raise ValueError("work directory must be absent or empty")
    work.mkdir(parents=True, exist_ok=True)
    result_path.parent.mkdir(parents=True, exist_ok=True)
    images = {
        "guest-kernel": pathlib.Path(args.kernel),
        "guest-initrd": pathlib.Path(args.initrd),
        "guest-root-disk": pathlib.Path(args.root_disk),
    }
    before = bind_images(pathlib.Path(args.comparison), images)
    binary = work / "boole-closed-local-mac-readiness-host"
    sdk = pathlib.Path(args.sdk).resolve()
    if not sdk.is_dir():
        raise ValueError("the selected macOS SDK is not one directory")
    module_cache = work / "swift-module-cache"
    module_cache.mkdir(mode=0o700)
    _run(
        swiftc_argv(
            pathlib.Path(args.swiftc), sdk, module_cache, binary
        )
    )
    _run(
        [
            args.codesign,
            "--force",
            "--sign",
            "-",
            "--entitlements",
            str(ENTITLEMENTS),
            str(binary),
        ]
    )
    dry_console = work / "dry-run.console"
    dry_receipt = work / "dry-run.receipt.json"
    _run(_host_argv(binary, before, dry_console, dry_receipt, args.timeout, True))
    dry = _json(dry_receipt)
    if dry.get("outcome") != "dry-run-configuration-valid" or dry.get("dryRun") is not True:
        raise ValueError("the Mac host dry run did not validate the exact configuration")

    if args.mode == "preflight":
        after = bind_images(pathlib.Path(args.comparison), images)
        result = make_result(
            mode="preflight",
            images_before=before,
            images_after=after,
            host_receipt=dry,
            assessment={"ready": False, "vmStarted": False},
            transcript_sha256=_sha256(dry_console),
        )
        _write_json(result_path, result)
        return result

    console = work / "boot.console"
    receipt_path = work / "boot.receipt.json"
    _run(_host_argv(binary, before, console, receipt_path, args.timeout, False))
    receipt = _json(receipt_path)
    transcript = console.read_text(encoding="utf-8", errors="replace")
    assessment = assess_readiness(transcript, receipt, before)
    after = bind_images(pathlib.Path(args.comparison), images)
    result = make_result(
        mode="boot",
        images_before=before,
        images_after=after,
        host_receipt=receipt,
        assessment=assessment,
        transcript_sha256=_sha256(console),
    )
    _write_json(result_path, result)
    return result


def parser():
    value = argparse.ArgumentParser()
    value.add_argument("mode", choices=("preflight", "boot"))
    value.add_argument("--comparison", required=True)
    value.add_argument("--kernel", required=True)
    value.add_argument("--initrd", required=True)
    value.add_argument("--root-disk", required=True)
    value.add_argument("--work", required=True)
    value.add_argument("--result", required=True)
    value.add_argument("--timeout", type=int, default=60)
    value.add_argument("--swiftc", default="swiftc")
    value.add_argument("--sdk", required=True)
    value.add_argument("--codesign", default="codesign")
    return value


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        result = execute(args)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print("closed-local-mac-readiness: refused: %s" % error, file=sys.stderr)
        return 2
    print("closed-local-mac-readiness: %s" % result["status"])
    return 0 if result["status"].endswith("PASS") else 1


if __name__ == "__main__":
    raise SystemExit(main())
