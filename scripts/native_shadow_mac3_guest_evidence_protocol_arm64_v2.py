"""Exact host-side comparisons for launcher-v2 console observations.

The v1 reader still owns the one-line framing and conflict rules.  This
successor fixes the payload schema that the new Rust producer must emit and
compares every observation with values sealed outside the guest.
"""

from scripts import native_shadow_mac3_guest_evidence_protocol_arm64_v1 as framing


PREFIX = framing.PREFIX
RECORDS = framing.RECORDS
format_record = framing.format_record
parse_line = framing.parse_line
read_transcript = framing.read_transcript

EXACT_PREREQUISITES = (
    "fixed-launcher-prelock-prerequisites",
    "fixed-launcher-lifetime-lock",
    "fresh-launcher-instance",
    "fixed-manager-cgroup",
    "fixed-startup-orphan-recovery",
    "fixed-startup-toolchain-compatibility",
    "runtime-rootfs-replay",
    "closed-local-replay-startup",
    "failed-unit-query",
)

FIXED_LAUNCHER_CAPABILITY_MASK = "00000000002001c0"

EXACT_SUPERVISOR = {
    "capabilitiesAmbient": "0000000000000000",
    "capabilitiesBounding": FIXED_LAUNCHER_CAPABILITY_MASK,
    "capabilitiesEffective": FIXED_LAUNCHER_CAPABILITY_MASK,
    "capabilitiesInheritable": "0000000000000000",
    "capabilitiesPermitted": FIXED_LAUNCHER_CAPABILITY_MASK,
    "gids": [0, 0, 0, 0],
    "noNewPrivileges": 0,
    "uids": [0, 0, 0, 0],
}


def _malformed_transcript(read: dict):
    malformed = read.get("malformed")
    if not isinstance(malformed, list):
        return False, "the transcript did not expose its malformed-record ledger"
    if malformed:
        return False, "the transcript contains a malformed guest evidence record"
    return None


def _exact_int(value):
    return isinstance(value, int) and not isinstance(value, bool)


def _exact_id_slots(value):
    return (
        isinstance(value, list)
        and len(value) == 4
        and all(_exact_int(item) for item in value)
    )


def _exact_capability(value):
    return (
        isinstance(value, str)
        and len(value) == 16
        and all(character in "0123456789abcdef" for character in value)
    )


def launcher_executable_matches(read: dict, *, expected_path: str, expected_sha256: str):
    refused = _malformed_transcript(read)
    if refused is not None:
        return refused
    record = read["records"].get("launcher-executable")
    if record is None:
        return False, "the guest did not report its executable"
    if set(record) != {"path", "sha256"}:
        return False, "the executable record has fields outside the exact schema"
    if record["path"] != expected_path:
        return False, "the guest executable path differs from the sealed guest path"
    if record["sha256"] != expected_sha256:
        return False, "the guest executable digest differs from the sealed launcher"
    return True, "the guest observed the exact sealed launcher path and digest"


def prerequisites_match(read: dict):
    refused = _malformed_transcript(read)
    if refused is not None:
        return refused
    record = read["records"].get("launcher-prerequisites")
    if record is None:
        return False, "the guest did not report its prerequisites"
    if set(record) != {"prerequisites"} or not isinstance(record["prerequisites"], list):
        return False, "the prerequisite record differs from the exact schema"
    rows = record["prerequisites"]
    if any(
        not isinstance(row, dict)
        or set(row) != {"name", "resolved"}
        or not isinstance(row["name"], str)
        or not isinstance(row["resolved"], bool)
        for row in rows
    ):
        return False, "the prerequisite rows require exact name and resolved boolean fields"
    expected = [{"name": name, "resolved": True} for name in EXACT_PREREQUISITES]
    if rows != expected:
        return False, (
            "the prerequisite rows must carry the exact ordered names and resolved booleans"
        )
    return True, "the guest observed all exact launcher-v2 prerequisites as resolved"


def supervisor_matches(read: dict):
    refused = _malformed_transcript(read)
    if refused is not None:
        return refused
    record = read["records"].get("supervisor-privilege")
    if record is None:
        return False, "the guest did not report the supervising privilege snapshot"
    if not isinstance(record, dict) or set(record) != set(EXACT_SUPERVISOR):
        return False, "the supervising privilege record differs from the exact schema"
    if not _exact_id_slots(record["uids"]) or not _exact_id_slots(record["gids"]):
        return False, "the supervising UID/GID slots are not exact integers"
    capability_fields = (
        "capabilitiesAmbient",
        "capabilitiesBounding",
        "capabilitiesEffective",
        "capabilitiesInheritable",
        "capabilitiesPermitted",
    )
    if any(not _exact_capability(record[field]) for field in capability_fields):
        return False, "a supervising capability set is not fixed lower-case hexadecimal"
    if not _exact_int(record["noNewPrivileges"]):
        return False, "NoNewPrivs is not an exact integer"
    if record != EXACT_SUPERVISOR:
        return False, (
            "the supervising UID/GID slots, five capability sets or NoNewPrivs differ"
        )
    return True, "the guest observed the complete fixed root-supervisor privilege shape"


def readiness_matches(read: dict):
    refused = _malformed_transcript(read)
    if refused is not None:
        return refused
    record = read["records"].get("readiness")
    if record is None:
        return False, "the guest did not report readiness"
    if set(record) != {"failedUnits", "ready"}:
        return False, "the readiness record differs from the exact schema"
    if record["ready"] is not True:
        return False, "the launcher did not reach its readiness point"
    if record["failedUnits"] != []:
        return False, "the fixed systemd query observed failed units"
    return True, "the launcher reached readiness and observed no failed systemd units"
