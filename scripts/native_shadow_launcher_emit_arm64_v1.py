#!/usr/bin/env python3
"""Hand the arm64 produce phase a launcher binary it is allowed to place.

`/usr/libexec/boole` is a closure root no package fills.  The launcher that goes
there is a build product, so it cannot arrive as a tracked repository file and
it cannot arrive as a deb, and the sealed producer authority names the only way
it may arrive instead: `rebuild-and-match-seal`.

That rule is the whole of this module.  It rebuilds the launcher through the
frozen build authority -- the same export, the same pinned sources, the same
locked offline cargo invocation -- and then believes the result only because it
reproduces a digest that two other records already sealed independently: the
producer authority and the double-build result.  A rebuild that does not
reproduce it is reported as the authority's own `launcher-digest-mismatch`,
never written and never retried until it happens to agree.

The proof that the launcher is reproducible is not made here.  It was made on
the arm64 runner by building twice and comparing, and it stays there; this is a
single build that has to answer to that proof.  Placing the emitted file into a
guest tree is a separate step again, and booting it is not any of these.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import sys
import tempfile
from typing import Any, Callable, Optional

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))

from scripts import native_shadow_launcher_build_arm64_v1 as build
from scripts import native_shadow_rootfs_builder_boot_arm64_v1 as boot


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False
LAUNCHER_DEPLOYED_INTO_GUEST = False

REPOSITORY_ROOT = pathlib.Path(__file__).resolve().parents[1]
BUILD_RESULT_PATH = (
    REPOSITORY_ROOT
    / "native/containment/native-shadow-launcher-build-result-arm64-v1.json"
)


class LauncherEmitError(RuntimeError):
    """The rebuilt launcher does not answer to the sealed one."""


def sealed_digest() -> str:
    """The digest both sealed records must already agree on.

    Two records sealed this launcher from different directions: the double build
    that proved it reproducible, and the producer authority that says what the
    produce phase may place.  Reading both and requiring them to match means a
    drift between them is found here rather than inherited.
    """

    try:
        built = json.loads(BUILD_RESULT_PATH.read_text(encoding="utf-8"))
    except OSError as exc:
        raise LauncherEmitError("the sealed launcher build result is unreadable") from exc
    launcher = built.get("launcher")
    if not isinstance(launcher, dict):
        raise LauncherEmitError("the sealed launcher build result seals no launcher")
    for field, expected in (
        ("sha256", boot.LAUNCHER_SHA256),
        ("sizeBytes", boot.LAUNCHER_SIZE_BYTES),
        ("guestLogicalPath", boot.LAUNCHER_GUEST_PATH),
    ):
        if launcher.get(field) != expected:
            raise LauncherEmitError(
                f"the two sealed launcher records disagree on {field}: "
                f"{launcher.get(field)!r} against {expected!r}"
            )
    return boot.LAUNCHER_SHA256


def rebuild(repo_root: pathlib.Path = REPOSITORY_ROOT) -> bytes:
    """One build through the frozen build authority, on the arm64 runner."""

    # The host is checked before the compiler is. A wrong-arch host would
    # otherwise compile a wrong-arch binary and fail as `launcher-digest-mismatch`,
    # which reads as a report about the launcher when it is one about the host.
    build._require_arm64_linux()
    authority = build.load_authority()
    drifted = build.verify_sources(authority, repo_root=repo_root)
    if drifted:
        raise LauncherEmitError(
            "pinned launcher source drifted: " + ", ".join(sorted(drifted))
        )
    build.prefetch(authority, repo_root=repo_root)
    with tempfile.TemporaryDirectory(prefix="boole-launcher-emit-") as scratch:
        return build.build_once(
            authority, workspace=pathlib.Path(scratch), repo_root=repo_root
        )


def emit(
    path: pathlib.Path,
    *,
    builder: Optional[Callable[[], bytes]] = None,
    sha256: Optional[str] = None,
    size: Optional[int] = None,
) -> dict[str, Any]:
    """Rebuild, match the seal, and write the launcher exactly once."""

    if path.exists():
        raise LauncherEmitError(f"refusing to overwrite an existing launcher at {path}")
    if sha256 is None and size is None:
        sealed_digest()
    binary = (builder or rebuild)()
    try:
        entry = boot.launcher_entry(binary, sha256=sha256, size=size)
    except boot.RootfsBuildError as exc:
        raise LauncherEmitError(str(exc)) from exc
    path.write_bytes(entry["raw"])
    os.chmod(path, entry["mode"])
    return {
        "activationAllowed": ACTIVATION_ALLOWED,
        "bootableClaim": BOOTABLE_CLAIM,
        "guestLogicalPath": boot.LAUNCHER_GUEST_PATH,
        "launcherDeployedIntoGuest": LAUNCHER_DEPLOYED_INTO_GUEST,
        "path": str(path),
        "sha256": boot.sha256_hex(entry["raw"]),
        "sizeBytes": len(entry["raw"]),
    }


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("emit", help="rebuild the launcher and match the seal")
    run.add_argument("--out", type=pathlib.Path, required=True)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        result = emit(args.out)
    except (LauncherEmitError, OSError) as exc:
        print(f"launcher-emit: {exc}", file=sys.stderr)
        return 1
    print(
        f"launcher {result['path']} bytes={result['sizeBytes']} "
        f"sha256={result['sha256']} matches the seal"
    )
    print(f"bootableClaim: {str(BOOTABLE_CLAIM).lower()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
