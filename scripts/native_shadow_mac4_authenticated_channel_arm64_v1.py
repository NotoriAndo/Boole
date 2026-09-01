#!/usr/bin/env python3
"""Run one reversible, closed-local MAC.4 authenticated vsock handshake."""

from __future__ import annotations

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

from scripts import native_shadow_closed_local_mac_readiness_arm64_v1 as readiness

RESULT_SCHEMA = "boole.native-shadow.mac4-authenticated-channel-result.arm64.v1"
CONTRACT = (
    REPO
    / "native/containment/native-shadow-mac4-authenticated-channel-contract-v1.json"
)
CONTRACT_SHA256 = "4f2ec110d72f628207ac383668daff7bda6b568449fd315d8376aeb20ae08bbd"
HOST_SOURCE = REPO / "native/mac4/boole-mac4-auth-channel.swift"
ENTITLEMENTS = REPO / "native/mac3/boole-mac3-closed-local-boot.entitlements"
KERNEL_COMMAND_LINE = "console=hvc0 root=/dev/vda ro init=/usr/lib/systemd/systemd"
EXACT_MACHINE = {
    "cpuCount": 2,
    "memoryBytes": 2 * 1024 * 1024 * 1024,
    "networkDevices": 0,
    "sharedDirectories": 0,
    "socketDevices": 1,
    "storageDevices": 1,
    "serialPorts": 1,
}


def _sha256(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while True:
            block = handle.read(4 * 1024 * 1024)
            if not block:
                return digest.hexdigest()
            digest.update(block)


def _require_contract() -> None:
    if _sha256(CONTRACT) != CONTRACT_SHA256:
        raise ValueError("MAC.4 authenticated-channel contract differs from its pin")


def boot_tuple_binding(bound_images) -> str:
    rows = {row["name"]: row for row in bound_images}
    if set(rows) != set(readiness.IMAGE_NAMES):
        raise ValueError("boot tuple does not name exactly three artifacts")
    digest = hashlib.sha256(b"boole.mac4.boot-tuple.v1\0")
    for name in readiness.IMAGE_NAMES:
        value = rows[name].get("sha256")
        if not isinstance(value, str) or len(value) != 64:
            raise ValueError("boot tuple digest is malformed")
        try:
            digest.update(bytes.fromhex(value))
        except ValueError as error:
            raise ValueError("boot tuple digest is malformed") from error
    return digest.hexdigest()


def host_argv(
    binary: pathlib.Path,
    bound_images,
    *,
    nonce_hex: str,
    binding_hex: str,
    console: pathlib.Path,
    receipt: pathlib.Path,
    timeout: int,
    dry_run: bool,
):
    rows = {row["name"]: row for row in bound_images}
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
        "--nonce-hex",
        nonce_hex,
        "--boot-binding-hex",
        binding_hex,
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


def _receipt_passes(receipt, images_before) -> bool:
    if receipt.get("schema") != "boole.native-shadow.mac4-authenticated-channel-run.v1":
        return False
    if receipt.get("outcome") != "authenticated-channel-pass":
        return False
    if receipt.get("dryRun") is not False or receipt.get("machine") != EXACT_MACHINE:
        return False
    if receipt.get("contractSha256") != CONTRACT_SHA256:
        return False
    if receipt.get("bootTupleBindingHex") != boot_tuple_binding(images_before):
        return False
    if not isinstance(receipt.get("nonceHex"), str) or len(receipt["nonceHex"]) != 64:
        return False
    if receipt.get("rootDisk", {}).get("attachedReadOnly") is not True:
        return False
    return receipt.get("vsock") == {"port": 4050, "handshakeComplete": True}


def make_result(
    *, mode, images_before, images_after, host_receipt, console_sha256
):
    unchanged = images_before == images_after
    authenticated = mode == "boot" and unchanged and _receipt_passes(
        host_receipt, images_before
    )
    if mode == "preflight":
        status = "MAC4-CHANNEL-PREFLIGHT-PASS"
    else:
        status = (
            "MAC4-AUTHENTICATED-CHANNEL-PASS"
            if authenticated
            else "MAC4-AUTHENTICATED-CHANNEL-FAIL"
        )
    return {
        "schema": RESULT_SCHEMA,
        "status": status,
        "mode": mode,
        "artifactClass": "DISPOSABLE-DEVELOPMENT",
        "contract": {"path": str(CONTRACT.relative_to(REPO)), "sha256": CONTRACT_SHA256},
        "imagesBefore": images_before,
        "imagesAfter": images_after,
        "imagesUnchanged": unchanged,
        "bootTupleBindingHex": boot_tuple_binding(images_before),
        "hostReceipt": host_receipt,
        "consoleTranscriptSha256": console_sha256,
        "channelAuthenticated": authenticated,
        "machinesStarted": 0 if mode == "preflight" else 1,
        "nodeConnected": False,
        "activationAllowed": False,
        "productionRelease": False,
        "publicMining": False,
        "rewardReady": False,
        "testnetClaim": False,
        "consensusTouched": False,
        "p2pTouched": False,
    }


def _json(path: pathlib.Path):
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError("host receipt is not one JSON object")
    return value


def _write_json(path: pathlib.Path, value) -> None:
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_text(
        json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    os.replace(temporary, path)


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


def execute(args):
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise ValueError("MAC.4 channel runner requires Apple Silicon macOS")
    if tuple(int(piece) for piece in platform.mac_ver()[0].split(".")[:1]) < (14,):
        raise ValueError("MAC.4 channel runner requires macOS 14 or newer")
    _require_contract()
    work = pathlib.Path(args.work).resolve()
    if work.exists() and any(work.iterdir()):
        raise ValueError("work directory must be absent or empty")
    work.mkdir(parents=True, exist_ok=True)
    result_path = pathlib.Path(args.result).resolve()
    result_path.parent.mkdir(parents=True, exist_ok=True)
    paths = {
        "guest-kernel": pathlib.Path(args.kernel),
        "guest-initrd": pathlib.Path(args.initrd),
        "guest-root-disk": pathlib.Path(args.root_disk),
    }
    before = readiness.bind_images(pathlib.Path(args.comparison), paths)
    binding = boot_tuple_binding(before)
    nonce = os.urandom(32).hex()
    binary = work / "boole-mac4-auth-channel-host"
    module_cache = work / "swift-module-cache"
    module_cache.mkdir(mode=0o700)
    subprocess.run(
        swiftc_argv(args.swiftc, pathlib.Path(args.sdk), module_cache, binary),
        check=True,
    )
    subprocess.run(
        [
            args.codesign,
            "--force",
            "--sign",
            "-",
            "--entitlements",
            str(ENTITLEMENTS),
            str(binary),
        ],
        check=True,
    )

    dry_console = work / "dry-run.console"
    dry_receipt = work / "dry-run.receipt.json"
    subprocess.run(
        host_argv(
            binary,
            before,
            nonce_hex=nonce,
            binding_hex=binding,
            console=dry_console,
            receipt=dry_receipt,
            timeout=args.timeout,
            dry_run=True,
        ),
        check=True,
    )
    dry = _json(dry_receipt)
    if dry.get("outcome") != "dry-run-configuration-valid" or dry.get("dryRun") is not True:
        raise ValueError("MAC.4 dry run did not validate the exact VM shape")

    if args.mode == "preflight":
        receipt = dry
        console = dry_console
    else:
        console = work / "boot.console"
        receipt_path = work / "boot.receipt.json"
        subprocess.run(
            host_argv(
                binary,
                before,
                nonce_hex=nonce,
                binding_hex=binding,
                console=console,
                receipt=receipt_path,
                timeout=args.timeout,
                dry_run=False,
            ),
            check=True,
        )
        receipt = _json(receipt_path)

    after = readiness.bind_images(pathlib.Path(args.comparison), paths)
    result = make_result(
        mode=args.mode,
        images_before=before,
        images_after=after,
        host_receipt=receipt,
        console_sha256=_sha256(console),
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
    value.add_argument("--timeout", type=int, default=90)
    value.add_argument("--swiftc", default="swiftc")
    value.add_argument("--sdk", required=True)
    value.add_argument("--codesign", default="codesign")
    return value


def main(argv=None):
    args = parser().parse_args(argv)
    try:
        result = execute(args)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print("MAC.4 authenticated channel refused: %s" % error, file=sys.stderr)
        return 2
    print("MAC.4 authenticated channel: %s" % result["status"])
    return 0 if result["status"].endswith("PASS") else 1


if __name__ == "__main__":
    raise SystemExit(main())
