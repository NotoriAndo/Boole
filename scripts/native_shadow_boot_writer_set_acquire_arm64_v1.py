#!/usr/bin/env python3
"""Bring the two sealed ext4 writer packages into the same content store.

The frozen closure's own acquirer cannot carry these.  Its plan is derived from
the sealed boot source lock and it requires that closure to be covered exactly
once, so adding anything to it would mean editing a sealed record -- which is a
stop condition, not a step.  The writer set therefore arrives on a path of its
own, and the separation is the point: the 191 guest packages are neither
replaced nor deleted, and nothing here can touch them because nothing here even
names them except to prove that it does not.

What is fetched was decided before anything was fetched.  A record sealed
earlier pinned the repository metadata, the signature it was verified with, and
each package's size and SHA-256; this tool reads that record and fetches those
two URLs, keeping only bytes that reproduce the digests already sealed.  The
server gets no vote: it cannot tell us what the right bytes are, only whether
it happens to be holding them.

Three things are checked before a request is made.  The digests have to agree
with the ones the production plan pins, so a drift between the two records
stops here rather than at image-writing time.  Neither digest and neither name
may be one the boot source lock already seals, so this can only ever be an
addition.  And the lock itself has to still hash to what the selection record
said it did, so "the guest packages are unchanged" is a measurement rather than
a sentence.

The request path is written out again here rather than borrowed.  The frozen
acquirer's own source bytes are pinned inside a sealed execution plan, so a
keyword added to it -- however carefully defaulted -- would put that plan out
of date, and bringing the plan up to date is editing a sealed record.  The
policy below is therefore the same policy, applied to one different snapshot:
one GET, no proxy, no redirect, no retry, no Range, TLS 1.2 or better with the
certificate and hostname checked, and a response whose length was declared
before it was read and matches the size sealed months ago.

Nothing here unpacks a package, writes an image or boots anything.  Holding
verified bytes is an input fact, and the result says so.
"""

from __future__ import annotations

import argparse
import hashlib
import http.client
import json
import pathlib
import ssl
import sys
import urllib.parse
from typing import Any, Callable, Iterable, Mapping, Optional

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts import native_shadow_boot_ci_payload_acquire_arm64_v1 as ci_payload
from scripts import native_shadow_boot_root_disk_arm64_v1 as root_disk
from scripts import native_shadow_boot_rootfs_payload_acquire_arm64_v1 as payload
from scripts import native_shadow_boot_writer_tree_arm64_v1 as writer_tree

canonical_json = payload.canonical_json


class WriterSetAcquisitionError(RuntimeError):
    """The writer set is not being brought in exactly as it was sealed."""


BOOTABLE_CLAIM = False
ACTIVATION_ALLOWED = False

CAS_RELATIVE_ROOT = ci_payload.CAS_RELATIVE_ROOT
CAS_ROOT = ci_payload.CAS_ROOT
BOOT_SOURCE_LOCK_PATH = (
    REPO_ROOT / "native/containment/native-shadow-boot-rootfs-source-lock-arm64-v1.json"
)

PLAN_SCHEMA = "boole.native-shadow.boot-writer-set-acquisition-plan.arm64.v1"
RESULT_SCHEMA = "boole.native-shadow.boot-writer-set-acquisition-result.arm64.v1"
RELEASE = "NATIVE-SHADOW-BOOT-WRITER-SET-ACQUISITION-ARM64-V1"
RESULT_STATUS = "WRITER-SET-PAYLOADS-PRESENT-VERIFIED-NOT-BOOT-AUTHORITY"

# Each package becomes one artifact under a name of its own.  The prefix is
# what keeps it out of the locked closure's namespace: a name the lock already
# uses would be a second artifact claiming a sealed identity.
ARTIFACT_ID_PREFIX = "writer-set-"

# A plan has done nothing, so none of its boundaries are raised.  The two that
# say what did not happen to the guest are false in the same literal sense --
# no guest package was replaced, none was deleted -- and stay false in the
# result, because the whole point of this path is that they never become true.
BOUNDARIES = {
    "bootAuthority": False,
    "deletesAGuestPackage": False,
    "imageBuilderAuthorityPresent": False,
    "replacesAGuestPackage": False,
    "writerSetPayloadsAcquired": False,
    "writerSetPayloadsVerified": False,
    "writerTreeMaterialized": False,
}
ACQUIRED_BOUNDARIES = dict(
    BOUNDARIES, writerSetPayloadsAcquired=True, writerSetPayloadsVerified=True
)

ABORT_CONDITIONS = (
    "artifact-name-collides-with-the-sealed-closure",
    "boot-source-lock-differs-from-what-the-selection-record-measured",
    "package-already-sealed-in-the-boot-source-lock",
    "response-differs-from-the-sealed-size-or-digest",
    "selection-record-and-production-plan-disagree",
    "selection-record-is-not-the-sealed-one",
    "url-outside-the-selected-snapshot",
)

NETWORK_POLICY = dict(ci_payload.NETWORK_POLICY)
SNAPSHOT_HOST = "snapshot.ubuntu.com"
REQUEST_TIMEOUT_SECONDS = 60
USER_AGENT = "boole-arm64-writer-set-acquirer-v1"


def _spec(value: Any, *, snapshot_prefix: str) -> dict[str, object]:
    """One fetch spec, admitting exactly one snapshot and nothing else.

    The prefix is an argument rather than a constant because the snapshot this
    acquirer talks to is named by the sealed record; it is still exactly one
    snapshot per call, and a URL that does not begin with it never becomes a
    request.
    """

    if not isinstance(value, dict) or set(value) != {
        "artifactId",
        "sha256",
        "sizeBytes",
        "url",
    }:
        raise WriterSetAcquisitionError("writer set spec keys differ")
    identifier = value["artifactId"]
    url = value["url"]
    if not isinstance(identifier, str) or not identifier:
        raise WriterSetAcquisitionError("writer set artifactId is invalid")
    if not isinstance(url, str) or not url:
        raise WriterSetAcquisitionError("writer set URL is invalid")
    parsed = urllib.parse.urlsplit(url)
    if (
        parsed.scheme != "https"
        or parsed.hostname != SNAPSHOT_HOST
        or parsed.port not in (None, 443)
        or parsed.username is not None
        or parsed.password is not None
        or parsed.query
        or parsed.fragment
        or not parsed.path.startswith(snapshot_prefix)
        or "\\" in parsed.path
        or "%" in parsed.path
        or "//" in parsed.path
        or ".." in pathlib.PurePosixPath(parsed.path).parts
    ):
        raise WriterSetAcquisitionError(
            "writer set URL violates the selected snapshot policy"
        )
    digest = value["sha256"]
    if (
        not isinstance(digest, str)
        or len(digest) != 64
        or any(character not in "0123456789abcdef" for character in digest)
    ):
        raise WriterSetAcquisitionError("writer set digest is not lowercase SHA-256")
    size = value["sizeBytes"]
    if isinstance(size, bool) or not isinstance(size, int) or size <= 0:
        raise WriterSetAcquisitionError("writer set size is invalid")
    return value


def _ordered_unique(
    specs: Iterable[dict[str, object]], *, snapshot_prefix: str
) -> list[dict[str, object]]:
    result = [_spec(value, snapshot_prefix=snapshot_prefix) for value in specs]
    identifiers = [str(value["artifactId"]) for value in result]
    digests = [str(value["sha256"]) for value in result]
    if len(identifiers) != len(set(identifiers)) or len(digests) != len(set(digests)):
        raise WriterSetAcquisitionError("writer set identity is duplicated")
    return sorted(result, key=lambda value: str(value["artifactId"]))


def snapshot_https_stream(
    spec: dict[str, object],
    *,
    snapshot_prefix: str,
    connection_factory: Any = http.client.HTTPSConnection,
    context_factory: Callable[[], ssl.SSLContext] = ssl.create_default_context,
) -> Iterable[bytes]:
    """Yield one exact snapshot response without proxy, redirect, retry or Range."""

    frozen = _spec(spec, snapshot_prefix=snapshot_prefix)
    parsed = urllib.parse.urlsplit(str(frozen["url"]))
    expected_size = int(frozen["sizeBytes"])
    context = context_factory()
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.verify_mode = ssl.CERT_REQUIRED
    context.check_hostname = True
    connection = connection_factory(
        SNAPSHOT_HOST, 443, timeout=REQUEST_TIMEOUT_SECONDS, context=context
    )
    observed = 0
    try:
        connection.putrequest(
            "GET", parsed.path, skip_host=True, skip_accept_encoding=True
        )
        connection.putheader("Host", SNAPSHOT_HOST)
        connection.putheader("Accept-Encoding", "identity")
        connection.putheader("Connection", "close")
        connection.putheader("User-Agent", USER_AGENT)
        connection.endheaders()
        response = connection.getresponse()
        if response.status != 200:
            raise WriterSetAcquisitionError("snapshot response status is not 200")
        encoding = response.getheader("Content-Encoding")
        if encoding not in (None, "identity"):
            raise WriterSetAcquisitionError("snapshot response encoding is forbidden")
        length = response.getheader("Content-Length")
        if length != str(expected_size):
            raise WriterSetAcquisitionError("snapshot Content-Length differs")
        while True:
            chunk = response.read(min(1024 * 1024, expected_size - observed + 1))
            if not chunk:
                break
            if not isinstance(chunk, bytes):
                raise WriterSetAcquisitionError("snapshot response chunk is invalid")
            observed += len(chunk)
            if observed > expected_size:
                raise WriterSetAcquisitionError("snapshot response exceeds sealed size")
            yield chunk
        if observed != expected_size:
            raise WriterSetAcquisitionError(
                "snapshot response is shorter than the sealed size"
            )
    except WriterSetAcquisitionError:
        raise
    except (OSError, TimeoutError, http.client.HTTPException) as exc:
        raise WriterSetAcquisitionError("snapshot request failed") from exc
    finally:
        connection.close()


def _read(path: pathlib.Path, context: str) -> bytes:
    try:
        return payload._read_regular_nofollow_stable(path, context)
    except payload.PayloadAcquisitionError as exc:
        raise WriterSetAcquisitionError(str(exc)) from exc


def _pin(raw: bytes) -> dict[str, Any]:
    return {"sha256": hashlib.sha256(raw).hexdigest(), "sizeBytes": len(raw)}


def _package_digests_the_production_plan_pins() -> dict[str, str]:
    """Which package each of the two production pins expects to come out of.

    The plan names the packages by role rather than by Debian name, so the two
    are lined up here by role and compared by digest.  A record that selected a
    different build would disagree with one of these and stop.
    """

    return {
        "writer": root_disk.WRITER_PACKAGE_SHA256,
        "library": root_disk.WRITER_LIBRARY_PACKAGE_SHA256,
    }


def _roles(packages: list[Mapping[str, Any]]) -> dict[str, Mapping[str, Any]]:
    """The package the writer comes out of, and the one its libraries do.

    The writer's package is the one whose name the selection record's own
    positive control was measured from; the other is the runtime library
    package.  Both are identified by what they carry rather than by position in
    the list, so a reordered record still lands the same way.
    """

    by_name = {str(row["name"]): row for row in packages}
    if set(by_name) != {"e2fsprogs", "libext2fs2t64"}:
        raise WriterSetAcquisitionError(
            "the selection record names packages this acquirer does not know: "
            + ", ".join(sorted(by_name))
        )
    return {"library": by_name["libext2fs2t64"], "writer": by_name["e2fsprogs"]}


def derive_plan(
    *,
    record: Optional[Mapping[str, Any]] = None,
    repository_root: pathlib.Path = REPO_ROOT,
) -> dict[str, Any]:
    """Work out, from the sealed record alone, what to fetch and from where."""

    if record is None:
        raw = _read(writer_tree.SELECTION_RECORD_PATH, "e2fsprogs selection record")
        try:
            record = payload._canonical_object(raw, "e2fsprogs selection record")
        except payload.PayloadAcquisitionError as exc:
            raise WriterSetAcquisitionError(str(exc)) from exc
    else:
        raw = canonical_json(record)

    lock_path = repository_root / BOOT_SOURCE_LOCK_PATH.relative_to(REPO_ROOT)
    lock_raw = _read(lock_path, "boot source lock")
    lock_pin = _pin(lock_raw)
    lock = json.loads(lock_raw.decode("utf-8"))

    guest = record["guestPackages"]
    if guest["sourceLockSha256"] != lock_pin["sha256"]:
        raise WriterSetAcquisitionError(
            "the boot source lock is not the one the selection record measured: "
            f"{lock_pin['sha256']} against {guest['sourceLockSha256']}"
        )
    if guest["replaced"] or guest["deleted"]:
        raise WriterSetAcquisitionError(
            "the selection record no longer says the guest packages are untouched"
        )

    selected = record["selection"]["selected"]
    index = record["writerToolSet"]["index"]
    base = str(index["snapshotBase"])
    prefix = urllib.parse.urlsplit(base).path + "/"

    expected = _package_digests_the_production_plan_pins()
    roles = _roles(list(record["writerToolSet"]["packages"]))
    specs = []
    for role, row in sorted(roles.items()):
        if str(row["sha256"]) != expected[role]:
            raise WriterSetAcquisitionError(
                f"the selection record and the production plan disagree about the "
                f"{role} package: {row['sha256']} against {expected[role]}"
            )
        if str(row["version"]) != str(selected["version"]):
            raise WriterSetAcquisitionError(
                f"the {role} package is version {row['version']}, but the record "
                f"selected {selected['version']}"
            )
        specs.append(
            {
                "artifactId": ARTIFACT_ID_PREFIX + str(row["name"]),
                "sha256": str(row["sha256"]),
                "sizeBytes": int(row["sizeBytes"]),
                "url": f"{base}/{row['poolPath']}",
            }
        )
    artifacts = _ordered_unique(specs, snapshot_prefix=prefix)

    locked_ids = {str(row["id"]) for row in lock["artifacts"]}
    locked_digests = {str(row["sha256"]) for row in lock["artifacts"]}
    for row in artifacts:
        if str(row["sha256"]) in locked_digests:
            raise WriterSetAcquisitionError(
                f"{row['artifactId']} is already sealed in the boot source lock; "
                "the writer set is added beside the guest closure, never into it"
            )
        if str(row["artifactId"]) in locked_ids:
            raise WriterSetAcquisitionError(
                f"{row['artifactId']} is a name the boot source lock already uses"
            )

    return {
        "abortConditions": list(ABORT_CONDITIONS),
        "activationAllowed": ACTIVATION_ALLOWED,
        "artifacts": artifacts,
        "authorityInputs": {
            "bootSourceLock": lock_pin,
            "selectionRecord": _pin(raw),
        },
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": dict(BOUNDARIES),
        "cas": {"layout": "sha256", "relativeRoot": CAS_RELATIVE_ROOT},
        "expected": {
            "fetchBytes": sum(int(row["sizeBytes"]) for row in artifacts),
            "fetchCount": len(artifacts),
        },
        "guestArtifactCount": int(guest["count"]),
        "networkPolicy": dict(NETWORK_POLICY, snapshotPathPrefix=prefix),
        "release": RELEASE,
        "schema": PLAN_SCHEMA,
        "selectedSuite": str(selected["suite"]),
        "selectedVersion": str(selected["version"]),
    }


def snapshot_stream_for(
    plan: Mapping[str, Any]
) -> Callable[[dict[str, object]], Iterable[bytes]]:
    """One request per artifact, admitting only the snapshot the plan names.

    The prefix travels in the plan rather than being a constant here, so the
    one snapshot this acquirer will talk to is the one the sealed record chose
    and not a second copy of it that could drift away.
    """

    prefix = str(plan["networkPolicy"]["snapshotPathPrefix"])
    return lambda spec: snapshot_https_stream(spec, snapshot_prefix=prefix)


def acquire(
    *,
    cas: pathlib.Path = CAS_ROOT,
    plan: Optional[Mapping[str, Any]] = None,
    repository_root: pathlib.Path = REPO_ROOT,
    result: Optional[pathlib.Path] = None,
    stream_factory: Optional[Callable[[dict[str, object]], Iterable[bytes]]] = None,
) -> dict[str, Any]:
    """Put the two sealed packages into the store, or stop and say why."""

    settled = derive_plan(repository_root=repository_root) if plan is None else plan
    fetch = snapshot_stream_for(settled) if stream_factory is None else stream_factory
    try:
        summary = ci_payload.acquire_specs(
            cas=cas, specs=settled["artifacts"], stream_factory=fetch
        )
    except (ci_payload.CiPayloadAcquisitionError, payload.PayloadAcquisitionError) as exc:
        raise WriterSetAcquisitionError(str(exc)) from exc

    document = {
        "activationAllowed": ACTIVATION_ALLOWED,
        "artifactIds": [str(row["artifactId"]) for row in settled["artifacts"]],
        "bootableClaim": BOOTABLE_CLAIM,
        "boundaries": dict(ACQUIRED_BOUNDARIES),
        "casRoot": str(cas),
        "planSha256": hashlib.sha256(canonical_json(settled)).hexdigest(),
        "release": RELEASE,
        "schema": RESULT_SCHEMA,
        "selectedVersion": str(settled["selectedVersion"]),
        "status": RESULT_STATUS,
        "summary": {
            "fetchedBytes": summary["fetchedBytes"],
            "fetchedCount": summary["fetched"],
            "reusedCount": summary["reused"],
        },
    }
    if result is not None:
        result.parent.mkdir(parents=True, exist_ok=True)
        result.write_bytes(canonical_json(document))
    return document


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("plan", help="print the plan the sealed record derives")
    run = sub.add_parser("acquire", help="bring the sealed writer set into the store")
    run.add_argument("--cas", type=pathlib.Path, default=CAS_ROOT)
    run.add_argument("--result", type=pathlib.Path)
    return parser


def main(argv: Optional[list[str]] = None) -> int:
    args = _parser().parse_args(argv)
    try:
        if args.command == "plan":
            sys.stdout.write(canonical_json(derive_plan()).decode("utf-8"))
            return 0
        document = acquire(cas=args.cas, result=args.result)
    except (
        WriterSetAcquisitionError,
        payload.PayloadAcquisitionError,
        OSError,
        KeyError,
        json.JSONDecodeError,
    ) as exc:
        print(f"writer-set-acquire: {exc}", file=sys.stderr)
        return 1
    summary = document["summary"]
    print(
        f"writer set present: fetched={summary['fetchedCount']} "
        f"reused={summary['reusedCount']} bytes={summary['fetchedBytes']}"
    )
    print(f"bootableClaim: {str(BOOTABLE_CLAIM).lower()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
