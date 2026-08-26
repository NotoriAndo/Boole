#!/usr/bin/env python3
"""Frozen build authority for the Linux/arm64 native-shadow launcher ELF.

The guest rootfs source lock defers exactly one role, `tracked-file:launcher-binary`,
because the launcher ELF is a build output and a digest cannot be stated for a file
that does not exist.  This module fixes every input that decides those bytes -- source
files, workspace manifests, toolchain identity, target triple, profile and build flags
-- and then builds the binary twice in two independent trees, refusing anything but a
byte-identical pair.

The build toolchain here is the *workspace* toolchain declared by `rust-toolchain.toml`,
not the frozen `rust-lang-ci` nightly distribution acquired for the guest.  Those are two
different toolchains serving two different purposes: the nightly compiles submitted proof
projects inside the guest at `/opt/boole/native-checker-toolchain`, while the launcher is
an ordinary workspace crate.  Conflating them would misattribute the launcher's provenance.

Determinism here is declared, never manufactured.  `--remap-path-prefix` maps each build's
own source root onto one logical root so the artifact does not encode where it was built;
that flag is written into the authority in the open.  Nothing suppresses a timestamp and
nothing normalizes an environment value beyond the declared set: if two builds disagree,
the disagreement is reported rather than smoothed away.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile
from typing import Any, Callable, Optional


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payload

canonical_json = payload.canonical_json

CONTAINMENT = REPO_ROOT / "native" / "containment"
TOOL_PATH = pathlib.Path(__file__).resolve()
AUTHORITY_PATH = CONTAINMENT / "native-shadow-launcher-build-authority-arm64-v1.json"
RESULT_PATH = CONTAINMENT / "native-shadow-launcher-build-result-arm64-v1.json"

AUTHORITY_SCHEMA = "boole.native-shadow.launcher-build-authority.arm64.v1"
RESULT_SCHEMA = "boole.native-shadow.launcher-build-result.arm64.v1"
RELEASE = "NATIVE-SHADOW-LAUNCHER-BUILD-ARM64-V1"
RESULT_STATUS = "LAUNCHER-ELF-BUILT-BYTE-IDENTICAL-NOT-BOOT-AUTHORITY"

AUTHORITY_KEYS = {
    "activationAllowed",
    "boundaries",
    "bootableClaim",
    "build",
    "determinism",
    "generator",
    "platform",
    "release",
    "schema",
    "sourceFiles",
    "toolchain",
}
SOURCE_KEYS = {"path", "sha256", "sizeBytes"}
GENERATOR_KEYS = {"path", "sha256"}
GENERATOR_PATH = "scripts/native_shadow_launcher_build_arm64_v1.py"
BOUNDARY_KEYS = {
    "bootAuthority",
    "guestImageBuilt",
    "imageBuilderAuthorityPresent",
    "kernelImageExtracted",
    "launcherDeployedIntoGuest",
    "runtimeCompatibilityVerified",
    "toolchainByteProvenanceClosed",
}

# The launcher is built by the workspace toolchain, installed in CI by a
# commit-pinned action rather than unpacked from an archive whose bytes we
# froze.  That is an honest gap, so `toolchainByteProvenanceClosed` stays false
# and the identity is asserted by probe output instead of by installed bytes.
TOOLCHAIN = {
    "byteProvenanceClosed": False,
    "channel": "1.95.0",
    "declaredBy": "rust-toolchain.toml",
    "identityProbeScope": "version-and-host-only;not-installed-byte-provenance",
    "installer": "dtolnay/rust-toolchain@3c5f7ea28cd621ae0bf5283f0e981fb97b8a7af9",
    "requiredCargoVersionPrefix": "cargo 1.95.0",
    "requiredHostTriple": "aarch64-unknown-linux-gnu",
    "requiredRustcVersionPrefix": "rustc 1.95.0",
}

PLATFORM = {
    "architecture": "aarch64",
    "os": "linux",
    "rustTarget": "aarch64-unknown-linux-gnu",
}

# `--locked` refuses a dependency resolution that differs from the committed
# lockfile; `--offline` refuses a network fetch during the build itself.
BUILD = {
    "artifactRelativePath": (
        "target/aarch64-unknown-linux-gnu/release/boole-native-shadow-launcher"
    ),
    "binary": "boole-native-shadow-launcher",
    "command": [
        "cargo",
        "build",
        "--locked",
        "--offline",
        "--release",
        "--target",
        "aarch64-unknown-linux-gnu",
        "-p",
        "boole-native-shadow-launcher",
        "--features",
        "linux-arm64-authority",
        "--bin",
        "boole-native-shadow-launcher",
    ],
    "features": ["linux-arm64-authority"],
    "guestLogicalPath": "/usr/libexec/boole/boole-native-shadow-launcher",
    # The linker decides bytes too, so it is named here rather than left out.
    # Its own bytes are whatever `cc` the runner image ships, which this project
    # has not frozen -- so the gap is stated instead of implied away.
    "linker": {
        "byteProvenanceClosed": False,
        "selection": "rustc-default-cc-driver-for-aarch64-unknown-linux-gnu",
    },
    "package": "boole-native-shadow-launcher",
    # Fetching is the one step allowed to touch the network, and it happens
    # before either build so that `--offline` can hold while code is compiled.
    "prefetchCommand": ["cargo", "fetch", "--locked"],
    "profile": "release",
    "profileFlags": {"overflowChecks": True, "panic": "abort"},
    "rustflags": ["--remap-path-prefix", "{sourceRoot}=/boole/launcher-build"],
}

DETERMINISM = {
    "artifactMustBeByteIdentical": True,
    "declaredEnvironment": {
        "CARGO_INCREMENTAL": "0",
        "CARGO_TERM_COLOR": "never",
        "LANG": "C",
        "LC_ALL": "C",
        "SOURCE_DATE_EPOCH": None,
        "TZ": "UTC",
    },
    "forbidEnvironmentNormalizationBeyondDeclared": True,
    "forbidTimestampSuppression": True,
    "independentBuildCount": 2,
    "mismatchAction": "report-the-difference-never-force-a-match",
    "sourceTreeOrigin": "git-archive-of-tracked-files-only",
}

AUTHORITY_SHA256 = "64f4ea0c6b574e1479e51a78e250da8fac6f3d3522d60cb03dde65b53da594ee"


class LauncherBuildError(RuntimeError):
    """The launcher build authority or one of its inputs is invalid."""


def sha256_bytes(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def launcher_build_authority_sha256(raw: bytes) -> str:
    """Digest this tool with its own authority pin blanked out.

    The pin names the document that names this tool, so a plain file digest can
    never equal it.  Blanking the literal breaks the cycle in a way both sides
    can reproduce.
    """

    marker = b'AUTHORITY_SHA256 = "'
    start = raw.index(marker) + len(marker)
    return sha256_bytes(raw[:start] + b"0" * 64 + raw[start + 64 :])


def _exact(value: Any, keys: set[str], context: str) -> dict[str, Any]:
    if not isinstance(value, dict) or set(value) != keys:
        raise LauncherBuildError(f"{context} keys differ from the frozen contract")
    return value


def _digest(value: Any, context: str) -> str:
    if not isinstance(value, str) or len(value) != 64:
        raise LauncherBuildError(f"{context} is not a sha256 digest")
    if value.lower() != value or any(c not in "0123456789abcdef" for c in value):
        raise LauncherBuildError(f"{context} is not a sha256 digest")
    return value


def load_authority(path: pathlib.Path = AUTHORITY_PATH) -> dict[str, Any]:
    raw = path.read_bytes()
    if canonical_json(json.loads(raw.decode("utf-8"))) != raw:
        raise LauncherBuildError("launcher build authority is not canonical JSON")
    if sha256_bytes(raw) != AUTHORITY_SHA256:
        raise LauncherBuildError("launcher build authority differs from the pin")
    return validate_authority(json.loads(raw.decode("utf-8")))


def validate_authority(authority: Any) -> dict[str, Any]:
    document = _exact(authority, AUTHORITY_KEYS, "authority")
    if document["schema"] != AUTHORITY_SCHEMA or document["release"] != RELEASE:
        raise LauncherBuildError("authority identity differs from the frozen contract")
    if document["activationAllowed"] is not False or document["bootableClaim"] is not False:
        raise LauncherBuildError("authority must not claim activation or boot")
    _validate_boundaries(document["boundaries"])
    if document["platform"] != PLATFORM:
        raise LauncherBuildError("platform differs from the frozen contract")
    if document["toolchain"] != TOOLCHAIN:
        raise LauncherBuildError("toolchain differs from the frozen contract")
    if document["toolchain"]["byteProvenanceClosed"] is not False:
        raise LauncherBuildError("toolchain byte provenance is not closed")
    if document["build"] != BUILD:
        raise LauncherBuildError("build recipe differs from the frozen contract")
    if document["determinism"] != DETERMINISM:
        raise LauncherBuildError("determinism contract differs from the frozen contract")
    _validate_generator(document["generator"])
    _validate_sources(document["sourceFiles"])
    return document


def _validate_generator(value: Any) -> dict[str, Any]:
    generator = _exact(value, GENERATOR_KEYS, "generator")
    _digest(generator["sha256"], "generator digest")
    if generator["path"] != GENERATOR_PATH:
        raise LauncherBuildError("generator path is not the frozen build tool")
    return generator


def _validate_boundaries(value: Any) -> None:
    boundaries = _exact(value, BOUNDARY_KEYS, "boundaries")
    for name, flag in boundaries.items():
        if flag is not False:
            raise LauncherBuildError(f"boundary {name} must stay false")


def _validate_sources(value: Any) -> list[dict[str, Any]]:
    if not isinstance(value, list) or not value:
        raise LauncherBuildError("sourceFiles is empty")
    paths: list[str] = []
    for row in value:
        entry = _exact(row, SOURCE_KEYS, "source file")
        _digest(entry["sha256"], "source file digest")
        size = entry["sizeBytes"]
        if not isinstance(size, int) or isinstance(size, bool) or size < 0:
            raise LauncherBuildError("source file size is not a byte count")
        path = entry["path"]
        if not isinstance(path, str) or not path or path.startswith("/") or ".." in path:
            raise LauncherBuildError("source file path is not repository-relative")
        paths.append(path)
    if paths != sorted(paths):
        raise LauncherBuildError("sourceFiles is not sorted by path")
    if len(set(paths)) != len(paths):
        raise LauncherBuildError("a source file path is duplicated")
    return value


def verify_sources(
    authority: dict[str, Any], *, repo_root: pathlib.Path = REPO_ROOT
) -> list[str]:
    """Re-hash every pinned source file and report the ones that drifted."""

    drifted: list[str] = []
    for row in authority["sourceFiles"]:
        candidate = repo_root / row["path"]
        if not candidate.is_file():
            drifted.append(row["path"])
            continue
        raw = candidate.read_bytes()
        if sha256_bytes(raw) != row["sha256"] or len(raw) != row["sizeBytes"]:
            drifted.append(row["path"])
    return drifted


def _require_arm64_linux() -> None:
    uname = os.uname()
    if uname.sysname != "Linux" or uname.machine not in {"aarch64", "arm64"}:
        raise LauncherBuildError(
            "the launcher build authority runs on Linux aarch64 only; "
            f"this host is {uname.sysname} {uname.machine}"
        )


def _probe(command: list[str]) -> str:
    completed = subprocess.run(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=60,
        env={"LANG": "C", "LC_ALL": "C", "PATH": os.environ.get("PATH", "/usr/bin:/bin")},
        close_fds=True,
    )
    if completed.returncode != 0:
        raise LauncherBuildError(f"toolchain probe failed: {' '.join(command)}")
    return completed.stdout.decode("utf-8", errors="replace")


def verify_toolchain(authority: dict[str, Any]) -> dict[str, str]:
    """Assert the probed toolchain identity, which is weaker than byte provenance."""

    toolchain = authority["toolchain"]
    rustc = _probe(["rustc", "-vV"])
    cargo = _probe(["cargo", "-vV"])
    if not rustc.startswith(toolchain["requiredRustcVersionPrefix"]):
        raise LauncherBuildError("rustc version differs from the frozen channel")
    if not cargo.startswith(toolchain["requiredCargoVersionPrefix"]):
        raise LauncherBuildError("cargo version differs from the frozen channel")
    host = ""
    for line in rustc.splitlines():
        if line.startswith("host: "):
            host = line[len("host: ") :].strip()
    if host != toolchain["requiredHostTriple"]:
        raise LauncherBuildError("rustc host triple differs from the frozen target")
    return {"cargo": cargo.splitlines()[0], "host": host, "rustc": rustc.splitlines()[0]}


def _build_environment(authority: dict[str, Any], source_root: pathlib.Path) -> dict[str, str]:
    declared = authority["determinism"]["declaredEnvironment"]
    environment = {
        name: value for name, value in declared.items() if value is not None
    }
    environment["PATH"] = os.environ.get("PATH", "/usr/bin:/bin")
    if "HOME" in os.environ:
        environment["HOME"] = os.environ["HOME"]
    if "CARGO_HOME" in os.environ:
        environment["CARGO_HOME"] = os.environ["CARGO_HOME"]
    if "RUSTUP_HOME" in os.environ:
        environment["RUSTUP_HOME"] = os.environ["RUSTUP_HOME"]
    flags = [
        part.replace("{sourceRoot}", str(source_root))
        for part in authority["build"]["rustflags"]
    ]
    environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    return environment


def _export_source_tree(destination: pathlib.Path, *, repo_root: pathlib.Path) -> None:
    """Materialize only git-tracked files.

    A directory copy would pick up ignored local debris -- `.DS_Store` sits in the
    launcher crate right now -- and make the exported tree host-dependent.
    """

    archive = destination / ".source.tar"
    with archive.open("wb") as handle:
        completed = subprocess.run(
            ["git", "archive", "--format=tar", "HEAD"],
            cwd=str(repo_root),
            stdin=subprocess.DEVNULL,
            stdout=handle,
            stderr=subprocess.PIPE,
            check=False,
            timeout=300,
        )
    if completed.returncode != 0:
        raise LauncherBuildError("git archive of the tracked source tree failed")
    shutil.unpack_archive(str(archive), str(destination), format="tar")
    archive.unlink()


def prefetch(authority: dict[str, Any], *, repo_root: pathlib.Path = REPO_ROOT) -> None:
    """Populate the shared registry once so both builds can compile offline."""

    completed = subprocess.run(
        authority["build"]["prefetchCommand"],
        cwd=str(repo_root),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=1800,
        close_fds=True,
    )
    if completed.returncode != 0:
        tail = completed.stderr.decode("utf-8", errors="replace")[-2000:]
        raise LauncherBuildError(f"dependency prefetch failed:\n{tail}")


def build_once(
    authority: dict[str, Any],
    *,
    workspace: pathlib.Path,
    repo_root: pathlib.Path = REPO_ROOT,
) -> bytes:
    """Export the tracked tree into `workspace`, build, and return the artifact bytes."""

    _export_source_tree(workspace, repo_root=repo_root)
    # `git archive` exports HEAD, while the pins were verified against the working
    # tree. Re-verifying inside the exported tree closes that gap directly: what is
    # about to be compiled is what was pinned, not merely what sat on disk.
    drifted = verify_sources(authority, repo_root=workspace)
    if drifted:
        raise LauncherBuildError(
            "exported source tree differs from the pinned sources: "
            + ", ".join(sorted(drifted))
        )
    completed = subprocess.run(
        authority["build"]["command"],
        cwd=str(workspace),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=3600,
        env=_build_environment(authority, workspace),
        close_fds=True,
    )
    if completed.returncode != 0:
        tail = completed.stderr.decode("utf-8", errors="replace")[-4000:]
        raise LauncherBuildError(f"launcher build failed:\n{tail}")
    artifact = workspace / authority["build"]["artifactRelativePath"]
    if not artifact.is_file():
        raise LauncherBuildError("launcher build produced no artifact at the frozen path")
    return artifact.read_bytes()


def build_twice(
    authority: dict[str, Any],
    *,
    repo_root: pathlib.Path = REPO_ROOT,
    builder: Optional[Callable[..., bytes]] = None,
) -> dict[str, Any]:
    """Build in two independent trees and refuse anything but identical bytes."""

    run = builder or build_once
    if builder is None:
        prefetch(authority, repo_root=repo_root)
    digests: list[str] = []
    sizes: list[int] = []
    count = authority["determinism"]["independentBuildCount"]
    for index in range(count):
        with tempfile.TemporaryDirectory(prefix=f"boole-launcher-build-{index}-") as raw:
            produced = run(authority, workspace=pathlib.Path(raw), repo_root=repo_root)
        digests.append(sha256_bytes(produced))
        sizes.append(len(produced))
    if len(set(digests)) != 1 or len(set(sizes)) != 1:
        # Report the disagreement; never retry until it happens to match.
        raise LauncherBuildError(
            "independent launcher builds are not byte-identical: "
            + ", ".join(f"build{i}={d}({s})" for i, (d, s) in enumerate(zip(digests, sizes)))
        )
    return {"buildCount": count, "sha256": digests[0], "sizeBytes": sizes[0]}


def build_result(
    authority: dict[str, Any], built: dict[str, Any], identity: dict[str, str]
) -> dict[str, Any]:
    return {
        "activationAllowed": False,
        "bootableClaim": False,
        "boundaries": dict(authority["boundaries"]),
        "authoritySha256": AUTHORITY_SHA256,
        "independentBuildCount": built["buildCount"],
        "launcher": {
            "guestLogicalPath": authority["build"]["guestLogicalPath"],
            "sha256": built["sha256"],
            "sizeBytes": built["sizeBytes"],
        },
        "observedToolchain": dict(identity),
        "release": RELEASE,
        "schema": RESULT_SCHEMA,
        "status": RESULT_STATUS,
    }


def seal_or_reprove(
    result: dict[str, Any], *, result_path: pathlib.Path = RESULT_PATH
) -> str:
    """Seal the first proof, re-prove every later one, overwrite none.

    The double build only runs on the arm64 runner, so the launcher digest is
    discovered there rather than authored here.  The first run writes the seal;
    every run after it must reproduce those exact bytes.  A divergence is the
    finding -- rewriting the seal to match the newer build would erase the only
    evidence that reproducibility broke.
    """

    raw = canonical_json(result)
    if result_path.exists():
        if result_path.read_bytes() != raw:
            raise LauncherBuildError(
                "this build disagrees with the sealed launcher build result; "
                "report the difference, never overwrite the seal"
            )
        return "re-proved"
    payload._write_result_once(result_path, raw)
    return "sealed"


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true", help="validate the authority only")
    group.add_argument("--build", action="store_true", help="build twice and seal the result")
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    authority = load_authority()
    drifted = verify_sources(authority)
    if drifted:
        raise LauncherBuildError(
            "pinned launcher source drifted: " + ", ".join(sorted(drifted))
        )
    if args.check:
        print(
            f"launcher build authority: {AUTHORITY_SHA256} "
            f"sources={len(authority['sourceFiles'])} "
            f"target={authority['platform']['rustTarget']} built=no"
        )
        return 0
    _require_arm64_linux()
    identity = verify_toolchain(authority)
    built = build_twice(authority)
    disposition = seal_or_reprove(build_result(authority, built, identity))
    print(
        f"launcher build: {RESULT_STATUS} builds={built['buildCount']} "
        f"identical=yes {disposition} sha256={built['sha256']} bytes={built['sizeBytes']}"
    )
    return 0


if __name__ == "__main__":  # pragma: no cover
    try:
        raise SystemExit(main())
    except LauncherBuildError as error:
        print(f"launcher build authority refused: {error}", file=sys.stderr)
        raise SystemExit(1) from error
