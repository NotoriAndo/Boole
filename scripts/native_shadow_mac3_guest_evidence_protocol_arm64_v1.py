"""The language the guest speaks on the console, and the host's reader for it.

Three of the five stopped conditions are properties of a running kernel: which
bytes the launcher actually is, which prerequisites resolve inside the guest, and
which capabilities the supervising process holds.  Nothing outside the machine
can see any of them, so the guest has to say them out loud, and the only way out
of a closed-local boot is the serial console the host already captures.

A console is a shared, noisy, line-interleaved place.  Kernel messages, systemd
messages and the guest's own records all land in it together, so the format here
is one line per record, carrying a fixed prefix the host can find and a payload
the host can parse.  A record split over two lines could be cut in half by
another writer between them, which is why none of them are.

What the host does with a record matters more than the format.  A record is an
observation, never a verdict: the guest reports the digest it computed and the
host compares it with the sealed value it has held all along.  A guest that
reported the wrong digest would fail its own condition, because the comparison
happens on the side that already knows the answer.  Nothing here lets a line on
a console assert that a condition passed.

Two things are deliberately refused.  A record that appears twice with different
payloads is not resolved by preferring one of them -- the host cannot tell which
run of the guest wrote which, so the pair is unusable and the condition reads as
unanswered.  And no record claims that submissions ran unprivileged: a boot that
never receives a request has no submission to watch, and a helper that printed
the claim anyway would be manufacturing the evidence rather than collecting it.
"""

import json
import re

PREFIX = "BOOLE-GUEST-EVIDENCE-1"

# Every record the guest may emit.  A record id outside this set is ignored
# rather than trusted: the reader must not grow new evidence sources because a
# console line asked it to.
RECORDS = (
    "launcher-executable",
    "launcher-prerequisites",
    "supervisor-privilege",
    "readiness",
)

# What each record is for, in the words of the condition it feeds.  Kept beside
# the ids so a reader of this file can see why any of them exist.
FEEDS = {
    "launcher-executable": "launcher-executable-matches-the-sealed-digest",
    "launcher-prerequisites": "launcher-prerequisites-verify-inside-the-guest",
    "supervisor-privilege": "launcher-supervises-as-root-and-submissions-run-unprivileged",
    "readiness": "readiness-and-clean-shutdown-are-observed",
}

WHY_THE_GUEST_CANNOT_ASSERT_A_PASS = (
    "A record carries what the guest observed, never what it concluded.  The "
    "host compares each observation with a value it sealed before the machine "
    "existed, so a wrong report fails the condition instead of passing it.  No "
    "line on the console is trusted to say that a condition was met."
)

WHY_SUBMISSIONS_ARE_NOT_CLAIMED = (
    "Half of the supervision condition is that a submission runs as an "
    "unprivileged account.  A closed boot receives no requests, so there is no "
    "submission to watch, and a record asserting it anyway would be manufactured "
    "rather than observed.  The helper reports the supervising process only, and "
    "the unobserved half stays unobserved."
)


class MalformedRecord(ValueError):
    """A line carried the prefix and then did not parse."""


def format_record(identifier: str, payload: dict) -> str:
    """Render one record as the single line the guest prints.

    Sorted keys and no newlines: the line has to be stable enough that two runs
    of the same guest produce the same text, and short enough that a console
    does not wrap it into something the reader has to reassemble.
    """

    if identifier not in RECORDS:
        raise MalformedRecord("%r is not a record this protocol defines" % identifier)
    body = json.dumps(payload, sort_keys=True, separators=(",", ":"))
    # An assertion of the invariant, not validation of the input: JSON escapes
    # line breaks, so this holds today.  It is checked anyway because the whole
    # reader depends on one record being one line, and a future change to how
    # the payload is serialised must fail here rather than in a transcript.
    if "\n" in body or "\r" in body:
        raise MalformedRecord("a record payload may not contain a line break")
    return "%s %s %s" % (PREFIX, identifier, body)


# The prefix may be preceded by a kernel timestamp, a systemd unit name, or
# anything else the console put in front of it, so the line is matched from the
# prefix onward rather than from its start.
_LINE = re.compile(r"%s\s+([a-z-]+)\s+(\{.*\})\s*$" % re.escape(PREFIX))


def parse_line(line: str):
    """Return (identifier, payload) for a record line, or None for anything else.

    Returning None for a non-record is not leniency -- the console is full of
    lines that are not records and every one of them is legitimately not our
    business.  A line that does carry the prefix and then fails to parse is a
    different thing entirely, and raises.
    """

    if PREFIX not in line:
        return None
    match = _LINE.search(line)
    if match is None:
        raise MalformedRecord("a line carries the prefix but no readable record")
    identifier, body = match.group(1), match.group(2)
    try:
        payload = json.loads(body)
    except ValueError as error:
        raise MalformedRecord("a record payload is not readable: %s" % error)
    if not isinstance(payload, dict):
        raise MalformedRecord("a record payload must be an object")
    return identifier, payload


def read_transcript(transcript: str) -> dict:
    """Collect every record in a console transcript, refusing the ambiguous ones.

    Three outcomes per record id, and they are not the same thing.  Absent means
    the guest never said it.  Present means it said it once.  Conflicting means
    it said it more than once with different answers, and there is no way from
    here to tell which one describes the run being judged -- so the record is
    dropped and its condition goes unanswered rather than being decided by which
    line happened to come last.
    """

    seen = {}
    conflicting = set()
    malformed = []
    unknown = set()
    for line in transcript.splitlines():
        try:
            found = parse_line(line)
        except MalformedRecord as error:
            malformed.append(str(error))
            continue
        if found is None:
            continue
        identifier, payload = found
        if identifier not in RECORDS:
            unknown.add(identifier)
            continue
        if identifier in seen and seen[identifier] != payload:
            conflicting.add(identifier)
        seen[identifier] = payload
    for identifier in conflicting:
        del seen[identifier]
    return {
        "records": seen,
        "conflicting": sorted(conflicting),
        "malformed": malformed,
        "unknownRecordIds": sorted(unknown),
        "missing": sorted(set(RECORDS) - set(seen) - conflicting),
        "submissionsObserved": False,
        "whySubmissionsAreNotClaimed": WHY_SUBMISSIONS_ARE_NOT_CLAIMED,
    }


def launcher_digest_matches(read: dict, sealed_sha256: str):
    """Compare what the guest said it executed with what was sealed."""

    record = read["records"].get("launcher-executable")
    if record is None:
        return False, "the guest never reported the digest of the file it executed"
    reported = record.get("sha256")
    if reported != sealed_sha256:
        return False, "the guest executed %s; the sealed digest is %s" % (
            reported,
            sealed_sha256,
        )
    return True, "the guest reported the sealed launcher digest %s" % sealed_sha256


def prerequisites_resolved(read: dict):
    """Every prerequisite the guest was asked about had to resolve inside it."""

    record = read["records"].get("launcher-prerequisites")
    if record is None:
        return False, "the guest never reported whether its prerequisites resolved"
    rows = record.get("prerequisites")
    if not rows:
        return False, "the guest reported an empty prerequisite list, which proves nothing"
    for row in rows:
        if not isinstance(row, dict) or set(row) != {"name", "resolved"}:
            return False, (
                "each prerequisite must carry exactly its name and resolved observation"
            )
        if not isinstance(row["name"], str) or not row["name"]:
            return False, "a prerequisite name is missing or unreadable"
        if not isinstance(row["resolved"], bool):
            return False, "a prerequisite resolved observation is not boolean"
    names = [row["name"] for row in rows]
    if len(set(names)) != len(names):
        return False, "the guest reported a prerequisite name more than once"
    absent = sorted(row["name"] for row in rows if not row["resolved"])
    if absent:
        return False, "the guest could not resolve %s" % ", ".join(absent)
    return True, "the guest resolved all %d prerequisites inside itself" % len(rows)


def supervises_as_root(read: dict):
    """The half of the supervision condition a closed boot can actually see.

    The other half -- that a submission runs unprivileged -- has no submission to
    watch here, so this returns the root half alone and says so.  Reporting it as
    the whole condition would be the quiet relaxation this design exists to
    avoid.
    """

    record = read["records"].get("supervisor-privilege")
    if record is None:
        return False, "the guest never reported the privilege of its supervising process"
    if record.get("uid") != 0:
        return False, "the supervising process runs as uid %r, not root" % record.get("uid")
    return True, (
        "the supervising process runs as root; the unprivileged-submission half "
        "is not observed by this boot and is not claimed here"
    )


def readiness_seen(read: dict):
    record = read["records"].get("readiness")
    if record is None:
        return False, "the guest never reported readiness"
    if not record.get("ready"):
        return False, "the guest reported that it was not ready"
    failed = record.get("failedUnits") or []
    if failed:
        return False, "the guest reported ready with %d failed unit(s)" % len(failed)
    return True, "the guest reported ready with no failed unit"
