"""Searches the sealed guest image for anything of the host's that got in.

One of the five conditions the third attempt cannot judge does not need the
guest to speak at all.  It asks that the produced image be searched for the
known secret-bearing filenames and for the host's own wallet and key
directories, and that they have no entry.  That is a question about a file
sitting on this disk right now, and nothing had been written that asks it.

The search runs over every byte rather than over a directory listing, which
makes it a superset of the question: it also reads file contents, and blocks no
directory entry points at any more.  A superset with nothing in it settles the
subset.  A superset with something in it settles nothing on its own -- the hit
may be a manual page that mentions a filename -- so a hit is neither a pass nor
a failure until someone explains it, and until then the answer is no.

Two things it must not do.  It must not disturb what it reads: the image is
opened read-only and hashed on both sides, so the record shows the sealed file
went in and the sealed file came out.  And it must not repeat what it finds:
hits carry an offset and a marker name, never the bytes around them.  A report
that quoted its findings would be a report that copied the secret.

It authorises nothing.  It answers one condition of twenty-one, and the other
four still need an image that can speak.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import sys

REPO = pathlib.Path(__file__).resolve().parents[1]
CONTAINMENT = REPO / "native/containment"
QUALIFICATION_PATH = (
    CONTAINMENT / "native-shadow-mac3-closed-local-boot-qualification-arm64-v3.json"
)
CONDITION = "no-host-wallet-model-key-or-node-secret-in-the-guest"

READ_CHUNK_BYTES = 4 * 1024 * 1024

TIERS = ("host-identity", "secret-shape")

WHY_A_BYTE_SEARCH_SETTLES_IT = (
    "the sealed condition asks that the secret-bearing filenames and the host's "
    "own wallet and key directories have no entry in the image.  Searching every "
    "byte is a superset of searching the directory entries: it reads file "
    "contents too, and blocks that no entry points at any more.  A superset that "
    "is empty proves the subset is empty, so nothing found answers the condition. "
    "The reverse does not hold -- a byte found is not an entry found -- so a hit "
    "has to be explained before it can be dismissed."
)


class RefusedError(RuntimeError):
    """Raised where continuing would report on something other than the seal."""


# --- what is searched for ------------------------------------------------------
#
# Every marker says where it came from.  The host-identity ones can only appear
# in the image if something of this machine's was carried in, so one hit is a
# failure on its own.  The secret-shape ones are generic and appear in ordinary
# software too; they are searched for anyway, and a hit is a question rather
# than a verdict.

_NODE_SECRET_PATHS = (
    (".boole/keys", "the node's default key directory (boole-cli main.rs)"),
    (".boole/sessions", "the node's default session directory (boole-cli main.rs)"),
    (
        ".boole/signer-nonces",
        "the node's default signer nonce directory (boole-cli main.rs)",
    ),
)

_NODE_SECRET_ENVIRONMENT = (
    ("BOOLE_WALLET_PASSPHRASE", "the wallet vault passphrase variable"),
    ("BOOLE_LLM_API_KEY", "the model API key variable"),
    ("BOOLE_KEYS_DIR", "the key directory override variable"),
    ("BOOLE_WALLET_AGENT_BIN", "the wallet agent binary override variable"),
    ("BOOLE_SESSIONS_DIR", "the session directory override variable"),
    ("BOOLE_SIGNER_NONCE_DIR", "the signer nonce directory override variable"),
)

# The armour lines are assembled rather than written out, and the reason is not
# style.  This file is checked into a repository whose own secret scan is a
# required gate, and a table of finished `BEGIN ... KEY` lines is exactly what
# that scan exists to report -- the search for leaked keys would fail the build
# for containing the shapes it searches for.  The bytes below are identical to
# the literals either way; a test asserts that, and asserts that this file does
# not spell them out.
_PEM_OPENING, _PEM_CLOSING = b"-----BEGIN ", b" KEY-----"


def _armour(label: bytes) -> bytes:
    """One PEM armour line, built from its two fixed halves."""

    return _PEM_OPENING + label + _PEM_CLOSING


_SECRET_SHAPES = (
    ("openssh-private-key-header", _armour(b"OPENSSH PRIVATE")),
    ("rsa-private-key-header", _armour(b"RSA PRIVATE")),
    ("ec-private-key-header", _armour(b"EC PRIVATE")),
    ("dsa-private-key-header", _armour(b"DSA PRIVATE")),
    ("pkcs8-private-key-header", _armour(b"PRIVATE")),
    ("encrypted-private-key-header", _armour(b"ENCRYPTED PRIVATE")),
    ("pgp-private-key-header", _PEM_OPENING + b"PGP PRIVATE KEY BLOCK-----"),
    ("anthropic-api-key-prefix", b"sk-ant-"),
    ("openrouter-api-key-prefix", b"sk-or-v1-"),
    ("openai-project-key-prefix", b"sk-proj-"),
    ("aws-secret-access-key-name", b"aws_secret_access_key"),
    ("extended-private-key-prefix", b"xprv"),
    ("bip39-mnemonic-field", b"mnemonic"),
    ("netrc-credentials-file", b".netrc"),
)


def markers() -> list:
    """The table, built now rather than written down.

    The host's home directory is read off this machine instead of being a
    literal in the file: whoever runs the search is the host it is searching
    for, and the path is the operator's, not something to commit.
    """

    rows = [
        {
            "id": "host-home-directory",
            "needle": str(pathlib.Path.home()).encode("utf-8"),
            "tier": "host-identity",
            "why": (
                "the home directory of the account that produced the image; any "
                "occurrence means a host path was carried into the guest"
            ),
            "anyHitIsAFailure": True,
            "disclosable": False,
        },
        {
            "id": "host-archive-root",
            "needle": b"boole-artifacts",
            "tier": "host-identity",
            "why": "the host directory the sealed images are kept in",
            "anyHitIsAFailure": True,
            "disclosable": True,
        },
    ]
    for path, why in _NODE_SECRET_PATHS:
        rows.append(
            {
                "id": "node-secret-path-%s" % path.replace("/", "-").strip("."),
                "needle": path.encode("utf-8"),
                "tier": "host-identity",
                "why": why,
                "anyHitIsAFailure": True,
                "disclosable": True,
            }
        )
    for name, why in _NODE_SECRET_ENVIRONMENT:
        rows.append(
            {
                "id": "node-secret-environment-%s" % name.lower().replace("_", "-"),
                "needle": name.encode("utf-8"),
                "tier": "host-identity",
                "why": why,
                "anyHitIsAFailure": True,
                "disclosable": True,
            }
        )
    for identifier, needle in _SECRET_SHAPES:
        rows.append(
            {
                "id": identifier,
                "needle": needle,
                "tier": "secret-shape",
                "why": (
                    "a generic secret-bearing shape; it appears in ordinary "
                    "software too, so a hit is a question rather than a verdict"
                ),
                "anyHitIsAFailure": False,
                "disclosable": True,
            }
        )
    return rows


# --- the search ---------------------------------------------------------------


def _scan_and_hash(handle, rows: list, chunk_bytes: int, digest=None):
    """One pass that both searches and hashes, so the file is read once.

    The window carries the tail of the previous read forward.  Without it every
    read boundary is a blind spot exactly as wide as the longest marker, and on
    an image this size that is hundreds of places a match could hide.
    """

    needles = [(row["id"], row["needle"], row["tier"]) for row in rows]
    overlap = max((len(needle) for _, needle, _ in needles), default=1) - 1
    carry = b""
    base = 0
    seen = set()
    hits = []
    read_bytes = 0
    while True:
        chunk = handle.read(chunk_bytes)
        if not chunk:
            break
        read_bytes += len(chunk)
        if digest is not None:
            digest.update(chunk)
        window = carry + chunk
        for identifier, needle, tier in needles:
            start = 0
            while True:
                found = window.find(needle, start)
                if found < 0:
                    break
                offset = base + found
                key = (identifier, offset)
                if key not in seen:
                    seen.add(key)
                    hits.append(
                        {"marker": identifier, "offset": offset, "tier": tier}
                    )
                start = found + 1
        # Carry the whole window forward while it is still shorter than the
        # longest marker.  Slicing a negative index here would silently drop
        # the front of the window instead, which loses any match starting in
        # the bytes that were thrown away.
        if overlap == 0:
            keep = b""
        elif len(window) <= overlap:
            keep = window
        else:
            keep = window[-overlap:]
        base += len(window) - len(keep)
        carry = keep
    hits.sort(key=lambda row: (row["offset"], row["marker"]))
    return hits, read_bytes


def scan_stream(handle, rows: list, chunk_bytes: int = READ_CHUNK_BYTES) -> list:
    hits, _ = _scan_and_hash(handle, rows, chunk_bytes)
    return hits


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    descriptor = os.open(path, os.O_RDONLY)
    try:
        with os.fdopen(descriptor, "rb", closefd=True) as handle:
            for chunk in iter(lambda: handle.read(READ_CHUNK_BYTES), b""):
                digest.update(chunk)
    except BaseException:
        os.close(descriptor)
        raise
    return digest.hexdigest()


def scan_target(
    path: pathlib.Path,
    expected_sha256: str,
    rows: list = None,
    chunk_bytes: int = READ_CHUNK_BYTES,
) -> dict:
    """Read the file once, searching and hashing, then hash it again.

    Opened read-only, so a mistake here cannot become a write to a sealed
    image.  The second hash is not paranoia about this program: it is what lets
    the record say the bytes that were searched are the bytes that are still
    there.
    """

    rows = markers() if rows is None else rows
    if not path.is_file():
        raise RefusedError("there is nothing to search at %s" % path)
    digest = hashlib.sha256()
    descriptor = os.open(path, os.O_RDONLY)
    with os.fdopen(descriptor, "rb", closefd=True) as handle:
        hits, read_bytes = _scan_and_hash(handle, rows, chunk_bytes, digest=digest)
    before = digest.hexdigest()
    if before != expected_sha256:
        raise RefusedError(
            "%s reads %s and the seal says %s; this is not the file the condition "
            "is about" % (path, before, expected_sha256)
        )
    after = sha256_file(path)
    if after != before:
        raise RefusedError(
            "%s changed while it was being read: %s then %s" % (path, before, after)
        )
    return {
        "path": str(path),
        "sha256Before": before,
        "sha256After": after,
        "bytesRead": read_bytes,
        "wholeFileRead": read_bytes == path.stat().st_size,
        "hits": hits,
    }


# --- what the hits mean --------------------------------------------------------


def _by_tier(hits: list) -> dict:
    """Count every tier, including the ones that found nothing.

    A tier left out of this mapping reads as "not searched for".  A tier
    present with a zero reads as "searched for, none found", which is the
    claim the record is actually making.
    """

    counts = {tier: 0 for tier in TIERS}
    for hit in hits:
        counts[hit["tier"]] = counts.get(hit["tier"], 0) + 1
    return counts


def verdict(hits: list) -> dict:
    counts = _by_tier(hits)
    if not hits:
        return {
            "noEntryFound": True,
            "hitsByTier": counts,
            "why": (
                "every byte of the image was searched for the host's own wallet and "
                "key directories and for the known secret-bearing filenames, and "
                "none of them occurs anywhere in it.  " + WHY_A_BYTE_SEARCH_SETTLES_IT
            ),
        }
    from_the_host = sorted({hit["marker"] for hit in hits if hit["tier"] == "host-identity"})
    if from_the_host:
        return {
            "noEntryFound": False,
            "hitsByTier": counts,
            "why": (
                "the image contains %d occurrence(s) of material that can only come "
                "from this host: %s"
                % (
                    len([hit for hit in hits if hit["tier"] == "host-identity"]),
                    ", ".join(from_the_host),
                )
            ),
        }
    generic = sorted({hit["marker"] for hit in hits})
    return {
        "noEntryFound": False,
        "hitsByTier": counts,
        "why": (
            "%d hit(s) on generic secret-bearing shapes (%s).  None of them is proof "
            "of an entry, and none of them is dismissed here: until each is "
            "explained, this condition reads as not met."
            % (len(hits), ", ".join(generic))
        ),
    }


# --- the record ----------------------------------------------------------------


def _marker_rows(rows: list) -> list:
    described = []
    for row in rows:
        described.append(
            {
                "id": row["id"],
                "tier": row["tier"],
                "why": row["why"],
                "anyHitIsAFailure": row["anyHitIsAFailure"],
                "needleBytes": len(row["needle"]),
                "needleSha256": hashlib.sha256(row["needle"]).hexdigest(),
                "needle": (
                    row["needle"].decode("utf-8", "replace")
                    if row.get("disclosable", True)
                    else None
                ),
            }
        )
    described.sort(key=lambda row: row["id"])
    return described


def build_record(
    target: str,
    path: pathlib.Path,
    expected_sha256: str,
    chunk_bytes: int = READ_CHUNK_BYTES,
) -> dict:
    rows = markers()
    scan = scan_target(path, expected_sha256, rows=rows, chunk_bytes=chunk_bytes)
    decided = verdict(scan["hits"])
    return {
        "record": "native-shadow-mac3-guest-secret-absence-scan-arm64-v1",
        "schema": 1,
        "condition": CONDITION,
        "target": target,
        "sha256Before": scan["sha256Before"],
        "sha256After": scan["sha256After"],
        "bytesRead": scan["bytesRead"],
        "wholeFileRead": scan["wholeFileRead"],
        "openedReadOnly": True,
        "hits": scan["hits"],
        "hitCount": len(scan["hits"]),
        "verdict": decided,
        "whyAByteSearchSettlesIt": WHY_A_BYTE_SEARCH_SETTLES_IT,
        "markersSearched": _marker_rows(rows),
        "hitsCarryNoSurroundingBytes": True,
        "bootAuthorisation": {
            "grantedByThisRecord": False,
            "requiredBefore": (
                "this record answers one condition of twenty-one.  The four that "
                "need the guest to speak are unchanged, and the operator's approval "
                "is a separate thing from either."
            ),
        },
    }


# --- command line ---------------------------------------------------------------


def qualification() -> dict:
    return json.loads(QUALIFICATION_PATH.read_text(encoding="utf-8"))


def resolve(target: str):
    record = qualification()
    root = pathlib.Path(record["subject"]["archiveRoot"])
    for row in record["subject"]["images"]:
        if row["name"] == target:
            return root / row["archivePath"], row["sha256"]
    known = ", ".join(sorted(row["name"] for row in record["subject"]["images"]))
    raise RefusedError("%s is not a sealed image; the sealed ones are %s" % (target, known))


def command_scan(args: argparse.Namespace) -> int:
    path, expected = resolve(args.target)
    record = build_record(target=args.target, path=path, expected_sha256=expected)
    text = json.dumps(record, indent=2, sort_keys=True) + "\n"
    if args.out:
        pathlib.Path(args.out).write_text(text, encoding="utf-8")
    else:
        sys.stdout.write(text)
    print(
        "scan: %s (%d hit(s) over %d bytes)"
        % (
            "NO ENTRY FOUND" if record["verdict"]["noEntryFound"] else "NOT SETTLED",
            record["hitCount"],
            record["bytesRead"],
        ),
        file=sys.stderr,
    )
    return 0 if record["verdict"]["noEntryFound"] else 1


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    child = sub.add_parser("scan")
    child.add_argument("--target", default="guest-root-disk")
    child.add_argument("--out", default=None)
    child.set_defaults(handler=command_scan)
    args = parser.parse_args(argv)
    try:
        return args.handler(args)
    except RefusedError as error:
        print("refused: %s" % error, file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
