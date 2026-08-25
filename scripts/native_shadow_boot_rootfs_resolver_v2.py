#!/usr/bin/env python3
"""Deterministic dependency resolver for an ARM64 boot-rootfs candidate.

This is an append-only successor to the frozen runtime-rootfs v1 resolver.  It
adds only the Debian syntax needed to inspect a real Ubuntu minimal boot
candidate: native-index ``:any``/``:native`` qualifiers, architecture
restrictions, and explicit pins for ambiguous virtual packages.  It does not
claim that a dependency closure is bootable.
"""

from __future__ import annotations

import copy
import json
import re
from typing import Any

from scripts import native_shadow_rootfs_acquire_arm64_v1 as acquire_v1
from scripts import native_shadow_rootfs_builder_arm64_v1 as rootfs


SCHEMA = "boole.native-shadow.boot-rootfs-dependency-resolution.arm64.v2"
_ALTERNATIVE_RE = re.compile(
    r"^(?P<name>[a-z0-9][a-z0-9+.-]*)(?P<qualifier>:(?:any|native))?"
    r"(?:\s*\((?P<op><<|<=|=|>=|>>)\s*(?P<version>[^()\s]+)\))?"
    r"(?:\s*\[(?P<architectures>[^\[\]]+)\])?$"
)
_ARCHITECTURE_TOKEN_RE = re.compile(r"^!?[a-z0-9][a-z0-9-]*$")
_BUILD_PROFILE_RE = re.compile(r"\s<[^<>]+>(?:\s|$)")


class ResolverV2Error(ValueError):
    """The signed package metadata cannot be resolved without guessing."""


def canonical_json(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()


def _parse_alternative(expression: str) -> re.Match[str]:
    if _BUILD_PROFILE_RE.search(expression):
        raise ResolverV2Error(
            f"build profile dependency is unsupported for runtime closure: {expression}"
        )
    match = _ALTERNATIVE_RE.fullmatch(expression)
    if match is None:
        raise ResolverV2Error(f"dependency alternative is unsupported: {expression}")
    return match


def _split_groups(value: str) -> list[list[str]]:
    if not value:
        return []
    if _BUILD_PROFILE_RE.search(value):
        raise ResolverV2Error("build profile dependency is unsupported for runtime closure")
    groups: list[list[str]] = []
    for raw_group in value.split(","):
        alternatives = [item.strip() for item in raw_group.split("|")]
        if not alternatives or any(not item for item in alternatives):
            raise ResolverV2Error("Ubuntu dependency group syntax differs")
        for alternative in alternatives:
            _parse_alternative(alternative)
        groups.append(alternatives)
    return groups


def _architecture_pattern_matches(
    pattern: str, *, target_os: str, target_architecture: str
) -> bool:
    if pattern in {"any", target_architecture}:
        return True
    if pattern == f"{target_os}-any":
        return True
    if pattern == f"any-{target_architecture}":
        return True
    return False


def _applies(
    match: re.Match[str], *, target_os: str, target_architecture: str
) -> bool:
    raw = match.group("architectures")
    if raw is None:
        return True
    tokens = raw.split()
    if not tokens or any(_ARCHITECTURE_TOKEN_RE.fullmatch(item) is None for item in tokens):
        raise ResolverV2Error("architecture restriction syntax differs")
    positive = [item for item in tokens if not item.startswith("!")]
    negative = [item[1:] for item in tokens if item.startswith("!")]
    if positive and negative:
        raise ResolverV2Error(
            "architecture restriction must not mix positive and negative terms"
        )
    if any(
        _architecture_pattern_matches(
            item, target_os=target_os, target_architecture=target_architecture
        )
        for item in negative
    ):
        return False
    return not positive or any(
        _architecture_pattern_matches(
            item, target_os=target_os, target_architecture=target_architecture
        )
        for item in positive
    )


def _provided_version(candidate: dict[str, Any], requested_name: str) -> tuple[bool, str | None]:
    matches: list[str | None] = []
    provides = candidate.get("provides", "")
    if provides:
        for raw in provides.split(","):
            match = rootfs._DEPENDENCY_RE.fullmatch(raw.strip())
            if match is None or match.group("qualifier") is not None:
                raise ResolverV2Error("Ubuntu Provides syntax differs")
            if match.group("op") not in (None, "="):
                raise ResolverV2Error("Ubuntu versioned Provides must use equality")
            if match.group("name") == requested_name:
                matches.append(match.group("version"))
    if len(matches) > 1:
        raise ResolverV2Error(f"duplicate provided name: {requested_name}")
    return (bool(matches), matches[0] if matches else None)


def _matches(match: re.Match[str], candidate: dict[str, Any]) -> bool:
    # The signed input is one native ARM64 index plus Architecture: all.  A
    # :any qualifier therefore never grants permission to select foreign bytes,
    # and still requires the package to opt in with Multi-Arch: allowed.
    if match.group("qualifier") == ":any" and candidate.get("multiArch") != "allowed":
        return False
    name = match.group("name")
    if candidate["name"] == name:
        # A package's real Version always defines its direct identity.  A
        # same-name Provides entry must never overwrite it.
        provided_version = candidate["version"]
    else:
        is_provided, provided_version = _provided_version(candidate, name)
        if not is_provided:
            return False
    if match.group("op") is not None and provided_version is None:
        return False
    try:
        return rootfs._version_satisfies(
            provided_version or candidate["version"],
            match.group("op"),
            match.group("version"),
        )
    except rootfs.RootfsBuildError as exc:
        raise ResolverV2Error(str(exc)) from exc


def _validate_pins(value: dict[str, str]) -> dict[str, str]:
    if not isinstance(value, dict):
        raise ResolverV2Error("virtual provider pins must be an object")
    result: dict[str, str] = {}
    for name, provider in value.items():
        if (
            not isinstance(name, str)
            or not isinstance(provider, str)
            or not re.fullmatch(r"[a-z0-9][a-z0-9+.-]*", name)
            or not re.fullmatch(r"[a-z0-9][a-z0-9+.-]*", provider)
        ):
            raise ResolverV2Error("virtual provider pin syntax differs")
        result[name] = provider
    result = {name: result[name] for name in sorted(result)}
    return result


def resolve_package_closure_v2(
    packages_raw: bytes,
    seeds: list[str],
    repository_id: str,
    component: str,
    *,
    target_os: str,
    target_architecture: str,
    virtual_provider_pins: dict[str, str],
) -> dict[str, Any]:
    if target_os != "linux" or target_architecture != "arm64":
        raise ResolverV2Error("resolver v2 target must be linux/arm64")
    if seeds != sorted(set(seeds)) or not seeds:
        raise ResolverV2Error("seed set must be non-empty, sorted, and unique")
    pins = _validate_pins(virtual_provider_pins)

    try:
        rows = rootfs._deb822_stanzas(packages_raw)
        candidates = [
            acquire_v1._IMPL["_candidate"](
                raw, fields, repository_id, component
            )
            for raw, fields in rows
        ]
    except (rootfs.RootfsBuildError, acquire_v1.AcquisitionError) as exc:
        raise ResolverV2Error(str(exc)) from exc
    candidates.sort(
        key=lambda item: (
            item["name"],
            item["version"],
            item["architecture"],
            item["packageId"],
        )
    )
    identities = [item["packageId"] for item in candidates]
    if len(identities) != len(set(identities)):
        raise ResolverV2Error("package artifact identity is ambiguous")

    used_pins: set[str] = set()

    def choose(alternatives: list[str]) -> tuple[int, dict[str, Any]] | None:
        for alternative_index, expression in enumerate(alternatives):
            match = _parse_alternative(expression)
            if not _applies(
                match,
                target_os=target_os,
                target_architecture=target_architecture,
            ):
                continue
            name = match.group("name")
            matching = [item for item in candidates if _matches(match, item)]
            direct = [item for item in matching if item["name"] == name]
            pool = direct if direct else matching
            if len(pool) > 1:
                provider = pins.get(name)
                if provider is None:
                    raise ResolverV2Error(
                        f"ambiguous dependency resolution requires provider pin: {expression}"
                    )
                pinned = [item for item in pool if item["name"] == provider]
                if len(pinned) != 1:
                    raise ResolverV2Error(
                        f"provider pin does not select exactly one candidate: {name}={provider}"
                    )
                used_pins.add(name)
                pool = pinned
            if pool:
                return alternative_index, pool[0]
        # A group containing only restrictions for other architectures does
        # not apply to this target.  At least one applicable alternative with
        # no candidate is instead an unresolved dependency.
        applicable = [
            expression
            for expression in alternatives
            if _applies(
                _parse_alternative(expression),
                target_os=target_os,
                target_architecture=target_architecture,
            )
        ]
        if not applicable:
            return None
        raise ResolverV2Error(
            f"unresolved dependency group: {' | '.join(applicable)}"
        )

    selected: dict[str, dict[str, Any]] = {}
    seed_ids: list[str] = []
    pending: list[dict[str, Any]] = []
    for seed in seeds:
        direct = [item for item in candidates if item["name"] == seed]
        if len(direct) != 1:
            raise ResolverV2Error(f"seed package identity differs: {seed}")
        seed_ids.append(direct[0]["packageId"])
        pending.append(direct[0])

    while pending:
        package = pending.pop(0)
        if package["packageId"] in selected:
            continue
        copied = copy.deepcopy(package)
        resolutions: list[dict[str, Any]] = []
        for field, key in (("Depends", "depends"), ("Pre-Depends", "preDepends")):
            for group_index, alternatives in enumerate(_split_groups(copied[key])):
                decision = choose(alternatives)
                if decision is None:
                    continue
                alternative_index, chosen = decision
                resolutions.append(
                    {
                        "field": field,
                        "groupIndex": group_index,
                        "alternativeIndex": alternative_index,
                        "packageId": chosen["packageId"],
                    }
                )
                pending.append(chosen)
        copied["dependencyResolutions"] = sorted(
            resolutions, key=lambda item: (item["field"], item["groupIndex"])
        )
        selected[copied["packageId"]] = copied

    unused = sorted(set(pins) - used_pins)
    if unused:
        raise ResolverV2Error(f"unused provider pin: {unused}")
    return {
        "schema": SCHEMA,
        "target": {"architecture": target_architecture, "os": target_os},
        "policy": {
            "architectureRestrictionEvaluation": "linux-arm64",
            "dependencyFields": ["Depends", "Pre-Depends"],
            "foreignArchitectureSelection": "forbidden",
            "multiArchQualifier": "native-index-only",
            "providerSelection": "direct-then-explicit-pin-else-stop",
        },
        "virtualProviderPins": pins,
        "seedPackageIds": sorted(seed_ids),
        "packages": [selected[key] for key in sorted(selected)],
    }
