#!/usr/bin/env python3
"""Put the frozen arm64 boot closure into a clean runner's content store.

The acquirer that filled the developer Mac's store is a pre-registration
document as much as a tool: it pins the plan it will accept, the `gpgv` and
`zstd` binaries on that machine, the packages that were already in the store
before it began, and the exact number of requests it would issue.  All of that
is true about one run on one host, and none of it is true on a clean runner.
Rewriting those pins would be editing a sealed record after the fact, so they
are left exactly as they are and the closure reaches CI another way.

That way is not a new argument.  The sealed boot source lock already names all
197 artifacts by digest and size; the sealed dependency and acquisition records
already name where each of them came from.  This tool reads both, fetches those
exact URLs, and keeps only bytes that hash to the digest the lock already sealed
-- the same shape as the Rust distribution acquirer beside it.  Against the
original signed-metadata replay this trusts the server strictly less: there, a
signature decided what the digests should be; here they are already decided and
the server gets no vote at all.

One artifact is not fetched.  The Ubuntu archive keyring is opened out of a
package the lock also pins, and the bytes that come out have to hash to the
keyring digest the lock sealed.  The decompressor that opens it is recorded
rather than pinned, because its output must equal a sealed digest and so its
identity cannot decide anything -- while pinning one host's copy would refuse
the other host for a reason that has nothing to do with the bytes.

Three artifacts are not fetched here either: the Rust archives have their own
sealed acquirer, which runs first.  Their absence stops this one rather than
being quietly filled in.

Nothing here builds an image, extracts a kernel, runs a maintainer script or
boots anything.  A store full of verified bytes is an input fact: the result
raises the two boundaries that holding them earns and leaves the rest false.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import sys
from typing import Any, Callable, Iterable, Optional

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_kernel_extract_arm64_v1 as deb
from scripts import native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payload

canonical_json = payload.canonical_json


class CiPayloadAcquisitionError(RuntimeError):
    """The frozen closure is not being reproduced exactly."""


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

CONTAINMENT = REPO_ROOT / "native" / "containment"
PLAN_PATH = CONTAINMENT / "native-shadow-boot-ci-payload-acquisition-plan-arm64-v1.json"
CAS_RELATIVE_ROOT = "local-docs/native-shadow-runtime-rootfs-source-lock-v1/cas"
CAS_ROOT = REPO_ROOT / CAS_RELATIVE_ROOT

PLAN_SCHEMA = "boole.native-shadow.boot-ci-payload-acquisition-plan.arm64.v1"
RESULT_SCHEMA = "boole.native-shadow.boot-ci-payload-acquisition-result.arm64.v1"
RELEASE = "NATIVE-SHADOW-BOOT-CI-PAYLOAD-ACQUISITION-ARM64-V1"
RESULT_STATUS = "CLOSURE-PAYLOADS-PRESENT-VERIFIED-NOT-BOOT-AUTHORITY"

RUSTDIST_ACQUIRER = "scripts/native_shadow_boot_rustdist_acquire_arm64_v1.py"

# A plan has not done anything yet, so every one of its boundaries is false.
# The result raises exactly the two that holding verified bytes earns, and
# nothing else: having the payloads is not having an image, a kernel or a boot.
BOUNDARIES = {
    "bootAuthority": False,
    "imageBuilderAuthorityPresent": False,
    "kernelImageExtracted": False,
    "launcherElfPresent": False,
    "maintainerScriptsExecuted": False,
    "packagePayloadsAcquired": False,
    "packagePayloadsVerified": False,
    "runtimeCompatibilityVerified": False,
}
ACQUIRED_BOUNDARIES = dict(
    BOUNDARIES, packagePayloadsAcquired=True, packagePayloadsVerified=True
)

ABORT_CONDITIONS = (
    "cas-object-fails-its-frozen-digest",
    "derived-artifact-differs-from-the-lock",
    "locked-artifact-has-no-frozen-source",
    "plan-differs-from-the-sealed-authorities",
    "response-differs-from-the-frozen-size-or-digest",
    "reused-artifact-absent-from-the-store",
    "sealed-authorities-disagree-about-an-artifact",
    "url-outside-the-frozen-snapshot",
)

SEALED_AUTHORITIES = {
    "bootSourceLock": "native-shadow-boot-rootfs-source-lock-arm64-v1.json",
    "dependencyCandidateResult": (
        "native-shadow-boot-rootfs-dependency-candidate-result-arm64-v1.json"
    ),
    "runtimeAcquisitionPlan": "native-shadow-runtime-rootfs-acquisition-plan-arm64-v1.json",
}

NETWORK_POLICY = {
    "allowEnvironmentProxy": False,
    "allowRangeRequests": False,
    "allowRedirects": False,
    "allowRetries": False,
    "allowedHosts": ["snapshot.ubuntu.com"],
    "concurrency": 1,
    "httpsOnly": True,
    "maxArtifactBytes": 536870912,
    "maxTotalBytes": 2147483648,
    "minimumTlsVersion": "TLSv1.2",
    "requireCertificateValidation": True,
    "requireContentLengthMatch": True,
    "requireHostnameValidation": True,
}

TOOLS = {
    "fetch": "python-stdlib-tls-one-request-no-proxy-no-redirect-no-retry",
    "zstdIdentity": "recorded-in-the-result-never-pinned",
    "zstdToolRole": "zstd",
}


def _sealed(repository_root: pathlib.Path) -> tuple[dict[str, Any], dict[str, Any]]:
    """Read the three sealed documents and pin what was read."""

    documents: dict[str, Any] = {}
    pins: dict[str, Any] = {}
    for name, filename in SEALED_AUTHORITIES.items():
        raw = payload._read_regular_nofollow_stable(
            repository_root / "native" / "containment" / filename, name
        )
        documents[name] = payload._canonical_object(raw, name)
        pins[name] = {"sha256": hashlib.sha256(raw).hexdigest(), "sizeBytes": len(raw)}
    return documents, pins


def _locked_spec(
    locked: dict[str, dict[str, Any]],
    identifier: str,
    *,
    url: str,
    sha256: str,
    size: int,
) -> dict[str, object]:
    """One fetch spec, with the digest and size taken from the lock.

    The source document carries its own copy of both.  Requiring the two to
    agree means a drift between the sealed records stops here, rather than being
    inherited by whichever of them happened to be read first.
    """

    sealed = locked.get(identifier)
    if sealed is None:
        raise CiPayloadAcquisitionError(
            f"{identifier} is named as a source but the boot source lock does not seal it"
        )
    if sealed["sha256"] != sha256 or sealed["sizeBytes"] != size:
        raise CiPayloadAcquisitionError(
            f"the sealed authorities disagree about {identifier}"
        )
    return payload._spec(
        {
            "artifactId": identifier,
            "sha256": sealed["sha256"],
            "sizeBytes": sealed["sizeBytes"],
            "url": url,
        }
    )


def derive_plan(repository_root: pathlib.Path = REPO_ROOT) -> dict[str, Any]:
    """Work out, from the sealed documents alone, how each artifact arrives."""

    documents, pins = _sealed(repository_root)
    lock = documents["bootSourceLock"]
    candidate = documents["dependencyCandidateResult"]
    acquisition = documents["runtimeAcquisitionPlan"]
    locked = {str(row["id"]): row for row in lock["artifacts"]}

    base = payload.SNAPSHOT_BASE
    specs = [
        _locked_spec(
            locked,
            str(row["artifactId"]),
            url=f"{base}/{row['poolPath']}",
            sha256=str(row["artifactSha256"]),
            size=int(row["artifactSizeBytes"]),
        )
        for row in candidate["resolution"]["packages"]
    ]
    repository = acquisition["repository"]
    for key in ("inRelease", "packagesIndex"):
        entry = repository[key]
        specs.append(
            _locked_spec(
                locked,
                str(entry["artifactId"]),
                url=f"{base}/{entry['path']}",
                sha256=str(entry["sha256"]),
                size=int(entry["sizeBytes"]),
            )
        )
    artifacts = payload._ordered_unique(specs)

    # The keyring is a member of a package rather than a download of its own.
    # The package is resolved by digest, because the acquisition record names it
    # with an identifier of its own that the lock has never used.
    bootstrap = acquisition["keyringBootstrap"]
    by_digest = {str(row["sha256"]): str(row["id"]) for row in lock["artifacts"]}
    source_id = by_digest.get(str(bootstrap["sha256"]))
    if source_id is None:
        raise CiPayloadAcquisitionError(
            "the keyring bootstrap package is not sealed in the boot source lock"
        )
    if locked[source_id]["sizeBytes"] != int(bootstrap["sizeBytes"]):
        raise CiPayloadAcquisitionError(
            f"the sealed authorities disagree about {source_id}"
        )
    keyring_id = str(bootstrap["keyringArtifactId"])
    keyring = locked.get(keyring_id)
    if keyring is None:
        raise CiPayloadAcquisitionError(
            "the extracted keyring is not sealed in the boot source lock"
        )
    derived = [
        {
            "artifactId": keyring_id,
            "fromArtifactId": source_id,
            "memberPath": str(bootstrap["memberPath"]),
            "sha256": keyring["sha256"],
            "sizeBytes": keyring["sizeBytes"],
        }
    ]

    reused = []
    for row in acquisition["rustArtifacts"]:
        identifier = str(row["artifactId"])
        sealed = locked.get(identifier)
        if sealed is None:
            raise CiPayloadAcquisitionError(
                f"{identifier} is named as a source but the boot source lock does not seal it"
            )
        if sealed["sha256"] != str(row["sha256"]) or sealed["sizeBytes"] != int(
            row["sizeBytes"]
        ):
            raise CiPayloadAcquisitionError(
                f"the sealed authorities disagree about {identifier}"
            )
        reused.append(identifier)
    reused.sort()

    covered = [str(row["artifactId"]) for row in artifacts]
    covered += [str(row["artifactId"]) for row in derived] + reused
    if sorted(covered) != sorted(locked) or len(covered) != len(set(covered)):
        unreachable = sorted(set(locked) - set(covered))
        raise CiPayloadAcquisitionError(
            "the sealed closure is not covered exactly once: "
            + (f"{len(unreachable)} artifacts have no frozen source" if unreachable
               else "an artifact arrives by more than one route")
        )

    fetch_bytes = sum(int(row["sizeBytes"]) for row in artifacts)
    return {
        "abortConditions": list(ABORT_CONDITIONS),
        "activationAllowed": ACTIVATION_ALLOWED,
        "artifacts": artifacts,
        "authorityInputs": pins,
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": dict(BOUNDARIES),
        "cas": {"layout": "sha256", "relativeRoot": CAS_RELATIVE_ROOT},
        "derivedArtifacts": derived,
        "expected": {
            "artifactCount": len(locked),
            "derivedCount": len(derived),
            "fetchBytes": fetch_bytes,
            "fetchCount": len(artifacts),
            "reusedCount": len(reused),
            "totalBytes": sum(int(row["sizeBytes"]) for row in lock["artifacts"]),
        },
        "networkPolicy": dict(NETWORK_POLICY),
        "release": RELEASE,
        "reusedArtifactIds": reused,
        "schema": PLAN_SCHEMA,
        "tools": dict(TOOLS),
    }


def load_plan(
    path: pathlib.Path = PLAN_PATH, repository_root: pathlib.Path = REPO_ROOT
) -> dict[str, Any]:
    """Read the committed plan and require the sealed documents to still derive it."""

    raw = payload._read_regular_nofollow_stable(path, "ci payload acquisition plan")
    if raw != canonical_json(derive_plan(repository_root)):
        raise CiPayloadAcquisitionError(
            "the committed plan differs from what the sealed authorities derive"
        )
    return payload._canonical_object(raw, "ci payload acquisition plan")


def derive_member(
    *, package: bytes, member_path: str, zstd_path: pathlib.Path
) -> bytes:
    """Open one regular file out of a .deb, without unpacking the rest of it."""

    try:
        data = deb.decompress_zstd(deb.ar_member(package, deb.DATA_MEMBER), zstd_path)
        return deb.tar_member(data, "./" + member_path.lstrip("./"))
    except deb.KernelExtractError as exc:
        raise CiPayloadAcquisitionError(str(exc)) from exc


def acquire_specs(
    *,
    cas: pathlib.Path,
    specs: Iterable[dict[str, object]],
    stream_factory: Callable[[dict[str, object]], Iterable[bytes]],
) -> dict[str, int]:
    """Fetch what the store does not already hold, and verify what it does."""

    ordered = list(specs)
    fetched = 0
    reused = 0
    fetched_bytes = 0
    with payload._cas_lock(cas):
        directory = payload._open_sha_directory(cas, create=True)
        try:
            for spec in ordered:
                if payload._verify_name(directory, spec):
                    reused += 1
                    continue
                payload._store_stream(directory, spec, stream_factory(spec))
                if not payload._verify_name(directory, spec):
                    raise CiPayloadAcquisitionError(
                        f"{spec['artifactId']} did not survive publication into the store"
                    )
                fetched += 1
                fetched_bytes += int(spec["sizeBytes"])
        finally:
            os.close(directory)
    return {"fetched": fetched, "fetchedBytes": fetched_bytes, "reused": reused}


def publish(*, cas: pathlib.Path, spec: dict[str, object], raw: bytes) -> None:
    """Put already-verified bytes into the store under their own digest."""

    if hashlib.sha256(raw).hexdigest() != str(spec["sha256"]) or len(raw) != int(
        spec["sizeBytes"]
    ):
        raise CiPayloadAcquisitionError(
            f"{spec['artifactId']} does not reproduce the digest the lock sealed"
        )
    with payload._cas_lock(cas):
        directory = payload._open_sha_directory(cas, create=True)
        try:
            if not payload._verify_name(directory, spec):
                payload._store_stream(directory, spec, [raw])
                if not payload._verify_name(directory, spec):
                    raise CiPayloadAcquisitionError(
                        f"{spec['artifactId']} did not survive publication into the store"
                    )
        finally:
            os.close(directory)


def require_reused(
    *, cas: pathlib.Path, plan: dict[str, Any], repository_root: pathlib.Path = REPO_ROOT
) -> None:
    """The Rust archives belong to their own sealed acquirer, which runs first."""

    documents, _ = _sealed(repository_root)
    locked = {str(row["id"]): row for row in documents["bootSourceLock"]["artifacts"]}
    with payload._cas_lock(cas):
        directory = payload._open_sha_directory(cas, create=True)
        try:
            for identifier in plan["reusedArtifactIds"]:
                sealed = locked[str(identifier)]
                spec = {
                    "artifactId": identifier,
                    "sha256": sealed["sha256"],
                    "sizeBytes": sealed["sizeBytes"],
                }
                if not payload._verify_name(directory, spec):
                    raise CiPayloadAcquisitionError(
                        f"{identifier} is absent from the store; it is fetched by "
                        f"{RUSTDIST_ACQUIRER}, which runs before this one"
                    )
        finally:
            os.close(directory)


def acquire(
    *,
    cas: pathlib.Path = CAS_ROOT,
    zstd_path: pathlib.Path,
    plan_path: pathlib.Path = PLAN_PATH,
    repository_root: pathlib.Path = REPO_ROOT,
    result: Optional[pathlib.Path] = None,
    stream_factory: Callable[
        [dict[str, object]], Iterable[bytes]
    ] = payload.snapshot_https_stream,
) -> dict[str, Any]:
    """Bring the whole sealed closure into the store, or stop and say why."""

    plan = load_plan(plan_path, repository_root)
    require_reused(cas=cas, plan=plan, repository_root=repository_root)
    summary = acquire_specs(
        cas=cas, specs=plan["artifacts"], stream_factory=stream_factory
    )

    derived_summary = []
    for row in plan["derivedArtifacts"]:
        source = plan_source(plan, str(row["fromArtifactId"]))
        package = read_object(cas, source)
        raw = derive_member(
            package=package,
            member_path=str(row["memberPath"]),
            zstd_path=zstd_path,
        )
        publish(cas=cas, spec=row, raw=raw)
        derived_summary.append(
            {"artifactId": row["artifactId"], "fromArtifactId": row["fromArtifactId"]}
        )

    document = {
        "activationAllowed": ACTIVATION_ALLOWED,
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": dict(ACQUIRED_BOUNDARIES),
        "casRoot": str(cas),
        "derived": derived_summary,
        "hostTools": [deb._host_tool_row("zstd", zstd_path)],
        "planSha256": hashlib.sha256(canonical_json(plan)).hexdigest(),
        "release": RELEASE,
        "schema": RESULT_SCHEMA,
        "status": RESULT_STATUS,
        "summary": {
            "derivedCount": len(derived_summary),
            "fetchedBytes": summary["fetchedBytes"],
            "fetchedCount": summary["fetched"],
            "reusedCount": summary["reused"] + len(plan["reusedArtifactIds"]),
        },
    }
    if result is not None:
        result.parent.mkdir(parents=True, exist_ok=True)
        result.write_bytes(canonical_json(document))
    return document


def plan_source(plan: dict[str, Any], identifier: str) -> dict[str, object]:
    for row in plan["artifacts"]:
        if str(row["artifactId"]) == identifier:
            return row
    raise CiPayloadAcquisitionError(f"{identifier} is not one of the fetched artifacts")


def read_object(cas: pathlib.Path, spec: dict[str, object]) -> bytes:
    raw = payload._read_regular_nofollow_stable(
        cas / "sha256" / str(spec["sha256"]), str(spec["artifactId"])
    )
    if hashlib.sha256(raw).hexdigest() != str(spec["sha256"]):
        raise CiPayloadAcquisitionError(
            f"{spec['artifactId']} in the store fails its frozen digest"
        )
    return raw


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("plan", help="print the plan the sealed documents derive")
    run = sub.add_parser("acquire", help="bring the sealed closure into the store")
    run.add_argument("--cas", type=pathlib.Path, default=CAS_ROOT)
    run.add_argument("--zstd", type=pathlib.Path, required=True)
    run.add_argument("--result", type=pathlib.Path)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command == "plan":
            sys.stdout.write(canonical_json(derive_plan()).decode("utf-8") + "\n")
            return 0
        document = acquire(cas=args.cas, zstd_path=args.zstd, result=args.result)
    except (
        CiPayloadAcquisitionError,
        payload.PayloadAcquisitionError,
        OSError,
        json.JSONDecodeError,
    ) as exc:
        print(f"ci-payload-acquire: {exc}", file=sys.stderr)
        return 1
    summary = document["summary"]
    print(
        f"closure present: fetched={summary['fetchedCount']} "
        f"reused={summary['reusedCount']} derived={summary['derivedCount']} "
        f"bytes={summary['fetchedBytes']}"
    )
    print(f"bootableClaim: {str(BOOTABLE_CLAIM).lower()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
